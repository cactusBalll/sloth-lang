//! Driver for the AOT backend: `llc` + native linker.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::backend::llvm::LlvmOptions;
use crate::backend::{image, llvm, modules};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitKind {
    Llvm,
    Obj,
    Exe,
}

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub emit: EmitKind,
    pub llc: PathBuf,
    pub cc: PathBuf,
    pub opt_level: u8,
    pub target: Option<String>,
    pub runtime_lib: Option<PathBuf>,
    pub keep_temps: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            emit: EmitKind::Exe,
            llc: PathBuf::from("llc-21"),
            cc: PathBuf::from("cc"),
            opt_level: 2,
            target: None,
            runtime_lib: None,
            keep_temps: false,
        }
    }
}

/// Compile `source` to LLVM IR text.
pub fn emit_llvm(source: &str, options: &BuildOptions) -> Result<String, String> {
    #[cfg(not(feature = "nan-boxing"))]
    {
        let _ = (source, options);
        return Err("the AOT backend requires the `nan-boxing` feature".to_string());
    }
    #[cfg(feature = "nan-boxing")]
    {
        let program = modules::compile_program(source)?;
        let image_bytes = image::serialize(&program.roots)?;
        llvm::generate(
            &program.roots,
            &image_bytes,
            &LlvmOptions {
                triple: options.target.clone(),
                opt_level: options.opt_level,
            },
        )
    }
}

/// Compile `source` and write the requested artifact to `output`.
pub fn build_executable(source: &str, output: &Path, options: &BuildOptions) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    let ir = emit_llvm(source, options)?;
    if options.emit == EmitKind::Llvm {
        write_file(output, ir.as_bytes())?;
        return Ok(());
    }

    let ll_path = with_suffix(output, "sloth.ll");
    let obj_path = with_suffix(output, "sloth.o");
    write_file(&ll_path, ir.as_bytes())?;

    let mut llc = Command::new(&options.llc);
    llc.arg(format!("-O{}", options.opt_level.min(3)));
    llc.arg("-relocation-model=pic");
    llc.arg("-filetype=obj");
    if let Some(target) = &options.target {
        llc.arg(format!("-mtriple={target}"));
    }
    llc.arg("-o").arg(&obj_path).arg(&ll_path);
    run_command(&mut llc, "llc")?;

    if options.emit == EmitKind::Obj {
        if obj_path != output {
            std::fs::copy(&obj_path, output)
                .map_err(|e| format!("cannot copy object file to {}: {e}", output.display()))?;
        }
        cleanup(&[&ll_path, &obj_path], output, options.keep_temps);
        return Ok(());
    }

    let runtime = match &options.runtime_lib {
        Some(path) => path.clone(),
        None => find_runtime_lib()?,
    };

    let mut cc = Command::new(&options.cc);
    cc.arg(&obj_path).arg(&runtime).arg("-o").arg(output);
    if cfg!(target_os = "linux") {
        cc.arg("-lm").arg("-lpthread").arg("-ldl");
    }
    run_command(&mut cc, "linker")?;

    cleanup(&[&ll_path, &obj_path], output, options.keep_temps);
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn with_suffix(output: &Path, suffix: &str) -> PathBuf {
    let mut path = output.to_path_buf();
    path.set_extension(suffix);
    path
}

fn run_command(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("failed to start {what}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "{what} failed with {}\n{stdout}{stderr}",
            output.status
        ));
    }
    Ok(())
}

fn cleanup(paths: &[&PathBuf], output: &Path, keep: bool) {
    if keep {
        return;
    }
    for path in paths {
        if path.as_path() != output {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Find `libsloth_runtime.a` next to the current executable or in the usual
/// cargo target directories.
pub fn find_runtime_lib() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("libsloth_runtime.a"));
        }
    }
    candidates.push(manifest.join("target/sloth-runtime/debug/libsloth_runtime.a"));
    candidates.push(manifest.join("target/sloth-runtime/release/libsloth_runtime.a"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("target/sloth-runtime/debug/libsloth_runtime.a"));
        candidates.push(cwd.join("target/sloth-runtime/release/libsloth_runtime.a"));
        candidates.push(cwd.join("target/debug/libsloth_runtime.a"));
        candidates.push(cwd.join("target/release/libsloth_runtime.a"));
    }
    candidates.push(manifest.join("target/debug/libsloth_runtime.a"));
    candidates.push(manifest.join("target/release/libsloth_runtime.a"));
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("cannot find libsloth_runtime.a; build it with `cargo build -p sloth-runtime --target-dir target/sloth-runtime`, let `sloth build` auto-build it, or pass --runtime-lib".to_string())
}
