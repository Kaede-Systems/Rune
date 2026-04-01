use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::ExitCode;

use rune::build::{
    BuildOptions, build_executable, build_executable_llvm_with_options,
    build_executable_with_options, build_shared_library, build_static_library, emit_c_header,
    supported_targets, target_spec,
};
use rune::diagnostics::render_file_diagnostic;
use rune::ir::lower_program;
use rune::lexer::{TokenKind, lex};
use rune::llvm_backend::emit_assembly_file;
use rune::llvm_ir::emit_llvm_ir;
use rune::module_loader::{LoadedProgram, load_program_bundle_from_path};
use rune::optimize::optimize_program;
use rune::parser::parse_source;
use rune::semantic::{check_program_with_context, check_program_with_context_all};
use rune::toolchain::{
    detect_windows_dev_assets, find_packaged_ld_lld, find_packaged_ld64_lld,
    find_packaged_lld_link, find_packaged_llvm_tool, find_packaged_wasm_ld, find_packaged_wasmtime,
};
use rune::warnings::collect_warnings;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(usage());
    };

    match command.as_str() {
        "lex" => {
            let Some(path) = args.next() else {
                return Err("missing source file path\n\nUsage: rune lex <file.rn>".to_string());
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune lex`\n\nUsage: rune lex <file.rn>".to_string(),
                );
            }

            let source_path = PathBuf::from(&path);
            let source = fs::read_to_string(&source_path)
                .map_err(|error| format!("failed to read `{path}`: {error}"))?;
            let tokens = lex(&source).map_err(|error| {
                render_file_diagnostic(&source_path, &source, &error.message, error.span)
            })?;
            for token in tokens {
                println!(
                    "{:>4}:{:<4} {}",
                    token.span.line,
                    token.span.column,
                    display_kind(&token.kind)
                );
            }
            Ok(())
        }
        "parse" => {
            let Some(path) = args.next() else {
                return Err("missing source file path\n\nUsage: rune parse <file.rn>".to_string());
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune parse`\n\nUsage: rune parse <file.rn>"
                        .to_string(),
                );
            }

            let source_path = PathBuf::from(&path);
            let source = fs::read_to_string(&source_path)
                .map_err(|error| format!("failed to read `{path}`: {error}"))?;
            let program = parse_source(&source).map_err(|error| {
                render_file_diagnostic(&source_path, &source, &error.message, error.span)
            })?;
            println!("{program:#?}");
            Ok(())
        }
        "check" => {
            let Some(path) = args.next() else {
                return Err("missing source file path\n\nUsage: rune check <file.rn>".to_string());
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune check`\n\nUsage: rune check <file.rn>"
                        .to_string(),
                );
            }

            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let program = &bundle.program;
            let warnings = collect_warnings(&program);
            let checked = check_program_with_context_all(program)
                .map_err(|failures| render_loaded_diagnostics(&bundle, &failures))?;
            for warning in &warnings {
                eprintln!(
                    "warning: {} at line {}, column {}",
                    warning.message, warning.span.line, warning.span.column
                );
            }
            println!("ok: checked {} function(s)", checked.functions.len());
            Ok(())
        }
        "emit-asm" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune emit-asm <file.rn>".to_string()
                );
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune emit-asm`\n\nUsage: rune emit-asm <file.rn>"
                        .to_string(),
                );
            }

            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let mut program = bundle.program.clone();
            let _checked = check_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            optimize_program(&mut program);
            let asm = rune::codegen::emit_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            println!("{asm}");
            Ok(())
        }
        "emit-ir" => {
            let Some(path) = args.next() else {
                return Err("missing source file path\n\nUsage: rune emit-ir <file.rn>".to_string());
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune emit-ir`\n\nUsage: rune emit-ir <file.rn>"
                        .to_string(),
                );
            }

            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let mut program = bundle.program.clone();
            let _checked = check_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            optimize_program(&mut program);
            let ir = lower_program(&program);
            print!("{ir}");
            Ok(())
        }
        "emit-llvm-ir" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune emit-llvm-ir <file.rn>".to_string(),
                );
            };

            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune emit-llvm-ir`\n\nUsage: rune emit-llvm-ir <file.rn>"
                        .to_string(),
                );
            }

            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let mut program = bundle.program.clone();
            let _checked = check_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            optimize_program(&mut program);
            let llvm_ir = emit_llvm_ir(&program).map_err(|error| error.to_string())?;
            print!("{llvm_ir}");
            Ok(())
        }
        "emit-llvm-asm" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune emit-llvm-asm <file.rn> [--target triple]".to_string(),
                );
            };

            let mut target: Option<String> = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--target" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--target`\n\nUsage: rune emit-llvm-asm <file.rn> [--target triple]".to_string(),
                            );
                        };
                        target = Some(value);
                    }
                    _ => {
                        return Err(
                            "invalid arguments for `rune emit-llvm-asm`\n\nUsage: rune emit-llvm-asm <file.rn> [--target triple]".to_string(),
                        );
                    }
                }
            }

            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let mut program = bundle.program.clone();
            let _checked = check_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            optimize_program(&mut program);
            let target_info = target_spec(target.as_deref()).map_err(|error| error.to_string())?;
            let temp_dir = std::env::temp_dir().join(format!(
                "rune-emit-llvm-asm-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| format!("failed to compute temp timestamp: {error}"))?
                    .as_nanos()
            ));
            fs::create_dir_all(&temp_dir)
                .map_err(|error| format!("failed to create `{}`: {error}", temp_dir.display()))?;
            let output_path = temp_dir.join("out.s");
            emit_assembly_file(&program, target_info.triple, &output_path)
                .map_err(|error| error.to_string())?;
            let asm = fs::read_to_string(&output_path)
                .map_err(|error| format!("failed to read `{}`: {error}", output_path.display()))?;
            print!("{asm}");
            let _ = fs::remove_file(output_path);
            let _ = fs::remove_dir(temp_dir);
            Ok(())
        }
        "emit-c-header" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune emit-c-header <file.rn> [-o output.h]"
                        .to_string(),
                );
            };

            let mut output: Option<PathBuf> = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `-o`\n\nUsage: rune emit-c-header <file.rn> [-o output.h]"
                                    .to_string(),
                            );
                        };
                        output = Some(PathBuf::from(value));
                    }
                    _ => {
                        return Err(
                            "invalid arguments for `rune emit-c-header`\n\nUsage: rune emit-c-header <file.rn> [-o output.h]"
                                .to_string(),
                        );
                    }
                }
            }

            let source_path = PathBuf::from(&path);
            let output_path = output.unwrap_or_else(|| source_path.with_extension("h"));
            emit_c_header(&source_path, &output_path).map_err(|error| error.to_string())?;
            println!("wrote {}", output_path.display());
            Ok(())
        }
        "build" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [-o output]"
                        .to_string(),
                );
            };

            let mut output: Option<PathBuf> = None;
            let mut build_lib = false;
            let mut build_static_lib = false;
            let mut target: Option<String> = None;
            let mut build_options = BuildOptions::default();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--lib" => {
                        build_lib = true;
                    }
                    "--static-lib" => {
                        build_static_lib = true;
                    }
                    "--target" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `--target`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [-o output]".to_string());
                        };
                        target = Some(value);
                    }
                    "--link-lib" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `--link-lib`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [-o output]".to_string());
                        };
                        build_options.link_libs.push(value);
                    }
                    "--link-search" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `--link-search`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [-o output]".to_string());
                        };
                        build_options.link_search_paths.push(PathBuf::from(value));
                    }
                    "--link-arg" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `--link-arg`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [-o output]".to_string());
                        };
                        build_options.link_args.push(value);
                    }
                    "--link-c-source" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `--link-c-source`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]".to_string());
                        };
                        build_options.link_c_sources.push(PathBuf::from(value));
                    }
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `-o`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [-o output]".to_string());
                        };
                        output = Some(PathBuf::from(value));
                    }
                    _ => {
                        return Err("invalid arguments for `rune build`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]".to_string());
                    }
                }
            }

            if build_lib && build_static_lib {
                return Err(
                    "cannot combine `--lib` and `--static-lib`\n\nUsage: rune build <file.rn> [--lib | --static-lib] [--target triple] [-o output]"
                        .to_string(),
                );
            }

            let source_path = PathBuf::from(&path);
            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            check_program_with_context_all(&bundle.program)
                .map_err(|failures| render_loaded_diagnostics(&bundle, &failures))?;
            let target_info = target_spec(target.as_deref()).map_err(|error| error.to_string())?;
            let output_path = output.unwrap_or_else(|| {
                if build_lib {
                    source_path.with_extension(target_info.library_extension)
                } else if build_static_lib {
                    source_path.with_extension(target_info.static_library_extension)
                } else {
                    source_path.with_extension(target_info.exe_extension)
                }
            });
            if build_lib {
                build_shared_library(&source_path, &output_path, target.as_deref())
                    .map_err(|error| error.to_string())?;
            } else if build_static_lib {
                build_static_library(&source_path, &output_path, target.as_deref())
                    .map_err(|error| error.to_string())?;
            } else {
                build_executable_with_options(
                    &source_path,
                    &output_path,
                    target.as_deref(),
                    &build_options,
                )
                    .map_err(|error| error.to_string())?;
            }
            println!("built {}", output_path.display());
            Ok(())
        }
        "build-llvm" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune build-llvm <file.rn> [--target triple] [-o output]"
                        .to_string(),
                );
            };

            let source_path = PathBuf::from(&path);
            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            check_program_with_context_all(&bundle.program)
                .map_err(|failures| render_loaded_diagnostics(&bundle, &failures))?;
            let mut target: Option<String> = None;
            let mut output: Option<PathBuf> = None;
            let mut build_options = BuildOptions::default();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--target" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--target`\n\nUsage: rune build-llvm <file.rn> [--target triple] [-o output]"
                                    .to_string(),
                            );
                        };
                        target = Some(value);
                    }
                    "--link-lib" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--link-lib`\n\nUsage: rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [-o output]"
                                    .to_string(),
                            );
                        };
                        build_options.link_libs.push(value);
                    }
                    "--link-search" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--link-search`\n\nUsage: rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [-o output]"
                                    .to_string(),
                            );
                        };
                        build_options.link_search_paths.push(PathBuf::from(value));
                    }
                    "--link-arg" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--link-arg`\n\nUsage: rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]"
                                    .to_string(),
                            );
                        };
                        build_options.link_args.push(value);
                    }
                    "--link-c-source" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--link-c-source`\n\nUsage: rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]"
                                    .to_string(),
                            );
                        };
                        build_options.link_c_sources.push(PathBuf::from(value));
                    }
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `-o`\n\nUsage: rune build-llvm <file.rn> [--target triple] [-o output]"
                                    .to_string(),
                            );
                        };
                        output = Some(PathBuf::from(value));
                    }
                    _ => {
                        return Err(
                            "invalid arguments for `rune build-llvm`\n\nUsage: rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]"
                                .to_string(),
                        );
                    }
                }
            }
            let target_info = target_spec(target.as_deref()).map_err(|error| error.to_string())?;
            let output_path = output.unwrap_or_else(|| {
                let stem = source_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("out");
                let suffix = if target_info.exe_extension.is_empty() {
                    ".llvm".to_string()
                } else {
                    format!(".llvm.{}", target_info.exe_extension)
                };
                source_path.with_file_name(format!("{stem}{suffix}"))
            });
            build_executable_llvm_with_options(
                &source_path,
                &output_path,
                target.as_deref(),
                &build_options,
            )
                .map_err(|error| error.to_string())?;
            println!("built {}", output_path.display());
            Ok(())
        }
        "decompile" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing binary path\n\nUsage: rune decompile <binary> [--target triple]"
                        .to_string(),
                );
            };

            let mut target: Option<String> = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--target" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--target`\n\nUsage: rune decompile <binary> [--target triple]"
                                    .to_string(),
                            );
                        };
                        target = Some(value);
                    }
                    _ => {
                        return Err(
                            "invalid arguments for `rune decompile`\n\nUsage: rune decompile <binary> [--target triple]"
                                .to_string(),
                        );
                    }
                }
            }

            let disassembly = decompile_binary(Path::new(&path), target.as_deref())?;
            print!("{disassembly}");
            Ok(())
        }
        "targets" => {
            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune targets`\n\nUsage: rune targets".to_string(),
                );
            }
            for target in supported_targets() {
                println!(
                    "{}  exe=.{}  lib=.{}  static=.{}",
                    target.triple,
                    if target.exe_extension.is_empty() {
                        "<none>"
                    } else {
                        target.exe_extension
                    },
                    target.library_extension,
                    target.static_library_extension
                );
            }
            Ok(())
        }
        "run-wasm" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing wasm path\n\nUsage: rune run-wasm <file.wasm> [--host node|wasmtime] [program args...]"
                        .to_string(),
                );
            };

            let mut host = String::from("node");
            let mut program_args = Vec::new();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--host" => {
                        let Some(value) = args.next() else {
                            return Err(
                                "missing value after `--host`\n\nUsage: rune run-wasm <file.wasm> [--host node|wasmtime] [program args...]"
                                    .to_string(),
                            );
                        };
                        host = value;
                    }
                    _ => program_args.push(arg),
                }
            }

            let exit_code = run_wasm_module(Path::new(&path), &host, &program_args)?;
            std::process::exit(exit_code);
        }
        "toolchain" => {
            if args.next().is_some() {
                return Err(
                    "too many arguments for `rune toolchain`\n\nUsage: rune toolchain".to_string(),
                );
            }
            println!("Bundled LLVM tools:");
            print_tool("llc", find_packaged_llvm_tool("llc"));
            print_tool("opt", find_packaged_llvm_tool("opt"));
            print_tool("clang", find_packaged_llvm_tool("clang"));
            print_tool("llvm-ar", find_packaged_llvm_tool("llvm-ar"));
            print_tool("lld-link", find_packaged_lld_link());
            print_tool("ld.lld", find_packaged_ld_lld());
            print_tool("ld64.lld", find_packaged_ld64_lld());
            print_tool("wasm-ld", find_packaged_wasm_ld());
            print_tool("wasmtime", find_packaged_wasmtime());

            println!();
            println!("Windows dev assets:");
            if let Some(assets) = detect_windows_dev_assets() {
                let arm64_complete = assets.msvc_lib_arm64.is_some()
                    && assets.sdk_lib_ucrt_arm64.is_some()
                    && assets.sdk_lib_um_arm64.is_some();
                if arm64_complete {
                    println!("  status: x64 + arm64 ready");
                } else {
                    println!("  status: x64 ready, arm64 incomplete");
                }
                println!("  msvc root: {}", assets.msvc_root.display());
                println!("  msvc include: {}", assets.msvc_include.display());
                println!("  msvc lib x64: {}", assets.msvc_lib_x64.display());
                println!(
                    "  msvc lib arm64: {}",
                    assets
                        .msvc_lib_arm64
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "missing".to_string())
                );
                println!("  sdk include ucrt: {}", assets.sdk_include_ucrt.display());
                println!("  sdk include um: {}", assets.sdk_include_um.display());
                println!("  sdk lib ucrt x64: {}", assets.sdk_lib_ucrt_x64.display());
                println!("  sdk lib um x64: {}", assets.sdk_lib_um_x64.display());
                println!(
                    "  sdk lib ucrt arm64: {}",
                    assets
                        .sdk_lib_ucrt_arm64
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "missing".to_string())
                );
                println!(
                    "  sdk lib um arm64: {}",
                    assets
                        .sdk_lib_um_arm64
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "missing".to_string())
                );
            } else {
                println!("  status: missing");
            }
            Ok(())
        }
        "debug" => {
            let Some(path) = args.next() else {
                return Err(
                    "missing source file path\n\nUsage: rune debug <file.rn> [-o output]"
                        .to_string(),
                );
            };

            let mut output: Option<PathBuf> = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "-o" | "--output" => {
                        let Some(value) = args.next() else {
                            return Err("missing value after `-o`\n\nUsage: rune debug <file.rn> [-o output]".to_string());
                        };
                        output = Some(PathBuf::from(value));
                    }
                    _ => {
                        return Err(
                            "invalid arguments for `rune debug`\n\nUsage: rune debug <file.rn> [-o output]"
                                .to_string(),
                        );
                    }
                }
            }

            let source_path = PathBuf::from(&path);
            let bundle =
                load_program_bundle_from_path(Path::new(&path)).map_err(|error| error.render())?;
            let mut program = bundle.program.clone();
            let _checked = check_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;
            optimize_program(&mut program);
            let ir = lower_program(&program).to_string();
            let asm = rune::codegen::emit_program_with_context(&program).map_err(|failure| {
                render_loaded_diagnostic(
                    &bundle,
                    &failure.function_name,
                    &failure.error.message,
                    failure.error.span,
                )
            })?;

            let output_path = output.unwrap_or_else(|| default_debug_output_path(&source_path));
            build_executable(&source_path, &output_path, None)
                .map_err(|error| error.to_string())?;
            let run_path = resolve_run_path(&output_path)?;
            let run_output = Command::new(&run_path)
                .output()
                .map_err(|error| format!("failed to run `{}`: {error}", run_path.display()))?;

            println!("== IR ==");
            print!("{ir}");
            if !ir.ends_with('\n') {
                println!();
            }
            println!("== ASM ==");
            print!("{asm}");
            if !asm.ends_with('\n') {
                println!();
            }
            println!("== Build ==");
            println!("{}", output_path.display());
            println!("== Run stdout ==");
            print!("{}", String::from_utf8_lossy(&run_output.stdout));
            if !run_output.stdout.ends_with(b"\n") {
                println!();
            }
            println!("== Run stderr ==");
            print!("{}", String::from_utf8_lossy(&run_output.stderr));
            if !run_output.stderr.ends_with(b"\n") {
                println!();
            }
            println!("== Exit Code ==");
            println!("{}", run_output.status.code().unwrap_or(-1));
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn render_loaded_diagnostic(
    bundle: &LoadedProgram,
    function_name: &str,
    message: &str,
    span: rune::lexer::Span,
) -> String {
    let mut prelude = String::new();
    if let Some(path) = bundle.function_origins.get(function_name)
        && let Some(source) = bundle.sources.get(path)
    {
        let chain = render_import_chain(bundle, path);
        if !chain.is_empty() {
            prelude.push_str(&chain);
            prelude.push('\n');
        }
        prelude.push_str(&render_file_diagnostic(path, source, message, span));
        return prelude;
    }
    if let Some((path, source)) = bundle.sources.iter().next() {
        return render_file_diagnostic(path, source, message, span);
    }
    format!("{message} at line {}, column {}", span.line, span.column)
}

fn render_loaded_diagnostics(
    bundle: &LoadedProgram,
    failures: &[rune::semantic::SemanticFailure],
) -> String {
    failures
        .iter()
        .map(|failure| {
            render_loaded_diagnostic(
                bundle,
                &failure.function_name,
                &failure.error.message,
                failure.error.span,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_import_chain(bundle: &LoadedProgram, leaf_path: &Path) -> String {
    let mut entries = Vec::new();
    let mut current = leaf_path.to_path_buf();

    while let Some(site) = bundle.import_sites.get(&current) {
        entries.push(format!(
            "  {}:{}:{} imported `{}`",
            pretty_path(&site.importer_path),
            site.importer_span.line,
            site.importer_span.column,
            site.module_name
        ));
        if site.importer_path == bundle.entry_path {
            break;
        }
        current = site.importer_path.clone();
    }

    if entries.is_empty() {
        String::new()
    } else {
        entries.reverse();
        format!(
            "Traceback (most recent import last):\n{}",
            entries.join("\n")
        )
    }
}

fn pretty_path(path: &Path) -> String {
    let raw = path.display().to_string();
    raw.strip_prefix("\\\\?\\").unwrap_or(&raw).to_string()
}

fn usage() -> String {
    "Usage:\n  rune lex <file.rn>\n  rune parse <file.rn>\n  rune check <file.rn>\n  rune emit-ir <file.rn>\n  rune emit-llvm-ir <file.rn>\n  rune emit-llvm-asm <file.rn> [--target triple]\n  rune emit-c-header <file.rn> [-o output.h]\n  rune emit-asm <file.rn>\n  rune build <file.rn> [--lib | --static-lib] [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]\n  rune build-llvm <file.rn> [--target triple] [--link-lib name] [--link-search dir] [--link-arg arg] [--link-c-source file.c] [-o output]\n  rune decompile <binary> [--target triple]\n  rune run-wasm <file.wasm> [--host node|wasmtime] [program args...]\n  rune targets\n  rune toolchain\n  rune debug <file.rn> [-o output]".to_string()
}

fn display_kind(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Identifier(value) => format!("Identifier({value})"),
        TokenKind::Integer(value) => format!("Integer({value})"),
        TokenKind::String(value) => format!("String({value:?})"),
        other => format!("{other:?}"),
    }
}

fn default_debug_output_path(source_path: &Path) -> PathBuf {
    let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("rune_debug");
    parent.join(format!("{stem}.debug.exe"))
}

fn print_tool(name: &str, path: Option<PathBuf>) {
    match path {
        Some(path) => println!("  {name}: {}", path.display()),
        None => println!("  {name}: missing"),
    }
}

fn run_wasm_module(path: &Path, host: &str, program_args: &[String]) -> Result<i32, String> {
    let resolved_path = resolve_run_path(path)?;
    if !resolved_path.is_file() {
        return Err(format!(
            "wasm module not found: {}",
            resolved_path.display()
        ));
    }

    let mut command = match host {
        "node" => {
            let sidecar = resolved_path.with_extension("js");
            if !sidecar.is_file() {
                return Err(format!(
                    "required wasm host sidecar not found: {}\nrebuild the module with `rune build ... --target wasm32-unknown-unknown`",
                    sidecar.display()
                ));
            }
            let mut command = Command::new("node");
            command.arg(&sidecar);
            command
        }
        "wasmtime" => {
            let wasmtime = find_packaged_wasmtime().ok_or_else(|| {
                "packaged Wasmtime runtime not found: expected a bundled `wasmtime` binary under tools/wasmtime"
                    .to_string()
            })?;
            let cwd = env::current_dir()
                .map_err(|error| format!("failed to determine current directory: {error}"))?;
            let mut command = Command::new(&wasmtime);
            command.arg("run");
            command.arg("--argv0").arg("rune-wasi");
            command.arg("--dir").arg(format!("{}::.", cwd.display()));
            for (key, _) in env::vars() {
                command.arg("--env").arg(key);
            }
            command.arg(&resolved_path);
            command.args(program_args);
            command
        }
        other => {
            return Err(format!(
                "unsupported wasm host `{other}`; expected `node` or `wasmtime`"
            ));
        }
    };

    if host == "node" {
        command.arg(&resolved_path);
        command.args(program_args);
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to run wasm host: {error}"))?;
    Ok(status.code().unwrap_or(0))
}

fn resolve_run_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    Ok(cwd.join(path))
}

fn decompile_binary(path: &Path, target: Option<&str>) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("binary not found: {}", path.display()));
    }

    let llvm_objdump = find_packaged_llvm_tool("llvm-objdump")
        .ok_or_else(|| "packaged LLVM tool not found: llvm-objdump".to_string())?;
    let mut command = Command::new(&llvm_objdump);
    command
        .arg("-d")
        .arg("-C")
        .arg("--no-show-raw-insn")
        .arg(path);
    if let Some(target) = target {
        command.arg(format!("--triple={target}"));
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to run `{}`: {error}", llvm_objdump.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed with exit code {}{}",
            llvm_objdump.display(),
            output.status.code().unwrap_or(-1),
            if output.stderr.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", String::from_utf8_lossy(&output.stderr))
            }
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
