use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::llvm_ir::{LlvmIrError, emit_llvm_ir};
use crate::parser::Program;
use crate::toolchain::find_packaged_llvm_tool_for_target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlvmBackendError {
    pub message: String,
}

impl fmt::Display for LlvmBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlvmBackendError {}

impl From<LlvmIrError> for LlvmBackendError {
    fn from(value: LlvmIrError) -> Self {
        Self {
            message: value.message,
        }
    }
}

pub fn emit_object_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_object_file_from_ir(&llvm_ir, target_triple, output_path)?;
    Ok(llvm_ir)
}

pub fn emit_assembly_file(
    program: &Program,
    target_triple: &str,
    output_path: &Path,
) -> Result<String, LlvmBackendError> {
    let llvm_ir = emit_llvm_ir(program)?;
    emit_assembly_file_from_ir(&llvm_ir, target_triple, output_path)?;
    Ok(llvm_ir)
}

pub fn emit_object_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "obj")
}

pub fn emit_assembly_file_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
) -> Result<(), LlvmBackendError> {
    emit_codegen_artifact_from_ir(llvm_ir, target_triple, output_path, "asm")
}

fn emit_codegen_artifact_from_ir(
    llvm_ir: &str,
    target_triple: &str,
    output_path: &Path,
    filetype: &str,
) -> Result<(), LlvmBackendError> {
    let temp_dir = create_temp_dir()?;
    let input_path = temp_dir.join("rune.ll");
    let optimized_path = temp_dir.join("rune.opt.ll");
    fs::write(&input_path, llvm_ir).map_err(|error| LlvmBackendError {
        message: format!("failed to write temporary LLVM IR file: {error}"),
    })?;

    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| LlvmBackendError {
            message: format!("failed to create `{}`: {error}", parent.display()),
        })?;
    }

    let input_arg = input_path.to_string_lossy().into_owned();
    let optimized_arg = optimized_path.to_string_lossy().into_owned();
    let output_arg = output_path.to_string_lossy().into_owned();
    run_llvm_tool(
        target_triple,
        "opt",
        llvm_opt_args(target_triple, &input_arg, &optimized_arg),
    )?;
    run_llvm_tool(
        target_triple,
        "llc",
        llvm_codegen_args(target_triple, filetype, &optimized_arg, &output_arg),
    )?;

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(optimized_path);
    let _ = fs::remove_dir(temp_dir);
    Ok(())
}

fn run_llvm_tool<S, I>(target_triple: &str, tool_name: &str, args: I) -> Result<(), LlvmBackendError>
where
    S: AsRef<str>,
    I: IntoIterator<Item = S>,
{
    let tool = find_packaged_llvm_tool_for_target(tool_name, target_triple).ok_or_else(|| LlvmBackendError {
        message: format!("packaged LLVM tool not found: {tool_name}"),
    })?;
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    let output = Command::new(&tool)
        .args(&args)
        .output()
        .map_err(|error| LlvmBackendError {
            message: format!("failed to run `{}`: {error}", tool.display()),
        })?;

    if output.status.success() {
        return Ok(());
    }

    Err(LlvmBackendError {
        message: format!(
            "{} failed with exit code {}{}",
            tool.display(),
            output.status.code().unwrap_or(-1),
            if output.stderr.is_empty() {
                String::new()
            } else {
                format!("\n\n{}", String::from_utf8_lossy(&output.stderr))
            }
        ),
    })
}

fn llvm_codegen_args(
    target_triple: &str,
    filetype: &str,
    input_arg: &str,
    output_arg: &str,
) -> Vec<String> {
    let mut args = match target_triple {
        "avr-atmega328p-arduino-uno" => {
            vec!["-mtriple=avr".to_string(), "-mcpu=atmega328p".to_string()]
        }
        _ => vec![format!("-mtriple={target_triple}")],
    };
    args.push(format!("-filetype={filetype}"));
    args.push(match target_triple {
        "avr-atmega328p-arduino-uno" => "-O2".to_string(),
        _ => "-O3".to_string(),
    });
    args.push(input_arg.to_string());
    args.push("-o".to_string());
    args.push(output_arg.to_string());
    args
}

fn llvm_opt_args(target_triple: &str, input_arg: &str, output_arg: &str) -> Vec<String> {
    let pipeline = match target_triple {
        "avr-atmega328p-arduino-uno" => "default<Oz>,verify",
        _ => "verify",
    };
    vec![
        "-S".to_string(),
        format!("-passes={pipeline}"),
        input_arg.to_string(),
        "-o".to_string(),
        output_arg.to_string(),
    ]
}

fn create_temp_dir() -> Result<PathBuf, LlvmBackendError> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = env::temp_dir().join(format!("rune-llvm-{}-{stamp}-{unique}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|error| LlvmBackendError {
        message: format!(
            "failed to create temporary LLVM directory `{}`: {error}",
            dir.display()
        ),
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::{llvm_codegen_args, llvm_opt_args};

    #[test]
    fn avr_codegen_args_use_exact_cpu_and_size_optimization() {
        let args = llvm_codegen_args(
            "avr-atmega328p-arduino-uno",
            "obj",
            "input.ll",
            "output.o",
        );
        assert!(args.contains(&"-mtriple=avr".to_string()));
        assert!(args.contains(&"-mcpu=atmega328p".to_string()));
        assert!(args.contains(&"-O2".to_string()));
    }

    #[test]
    fn avr_opt_args_use_size_pipeline() {
        let args = llvm_opt_args("avr-atmega328p-arduino-uno", "input.ll", "output.ll");
        assert!(args.contains(&"-passes=default<Oz>,verify".to_string()));
    }
}
