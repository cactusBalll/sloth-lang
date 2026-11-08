use clap::{Parser, Subcommand, ValueEnum};
use sloth_lang_core::backend::driver::find_runtime_lib;
use sloth_lang_core::backend::{build_executable, BuildOptions, EmitKind};
use sloth_lang_core::{run_string, run_string_debug};
use std::io::Read;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

#[derive(Parser)]
#[command(version, about = "sloth-lang interpreter and AOT compiler", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// script path (legacy form: `sloth script.slt`)
    script: Option<PathBuf>,

    #[arg(short, long)]
    debug: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Interpret a script with the bytecode VM.
    Run {
        script: PathBuf,
        #[arg(short, long)]
        debug: bool,
    },
    /// Compile a script to LLVM IR, an object file, or a native executable.
    Build {
        script: PathBuf,
        /// Output path.  Defaults to the script stem with the appropriate extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// What to emit.
        #[arg(long, value_enum, default_value_t = EmitArg::Exe)]
        emit: EmitArg,
        /// Path to llc (LLVM static compiler).
        #[arg(long, default_value = "llc-21")]
        llc: PathBuf,
        /// Path to the C linker driver.
        #[arg(long, default_value = "cc")]
        cc: PathBuf,
        /// llc optimization level (0-3).
        #[arg(long, default_value_t = 2)]
        opt_level: u8,
        /// LLVM target triple.  Defaults to the host target.
        #[arg(long)]
        target: Option<String>,
        /// Path to libsloth_runtime.a.
        #[arg(long)]
        runtime_lib: Option<PathBuf>,
        /// Keep the intermediate .ll and .o files.
        #[arg(long)]
        keep_temps: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum EmitArg {
    Llvm,
    Obj,
    Exe,
}

impl From<EmitArg> for EmitKind {
    fn from(value: EmitArg) -> Self {
        match value {
            EmitArg::Llvm => EmitKind::Llvm,
            EmitArg::Obj => EmitKind::Obj,
            EmitArg::Exe => EmitKind::Exe,
        }
    }
}

fn read_script(path: &PathBuf) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(buffer)
}

fn default_output(script: &PathBuf, emit: EmitKind) -> PathBuf {
    match emit {
        EmitKind::Llvm => script.with_extension("ll"),
        EmitKind::Obj => script.with_extension("o"),
        EmitKind::Exe => script.with_extension(""),
    }
}

/// Build `libsloth_runtime.a` on demand when the user did not provide one.
fn ensure_runtime_lib(emit: EmitKind, runtime_lib: &Option<PathBuf>) -> Result<(), String> {
    if emit != EmitKind::Exe || runtime_lib.is_some() || find_runtime_lib().is_ok() {
        return Ok(());
    }
    eprintln!("libsloth_runtime.a not found; building sloth-runtime ...");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/sloth-runtime");
    let status = ProcessCommand::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "build",
            "-p",
            "sloth-runtime",
            "--target-dir",
            target_dir.to_str().unwrap_or("target/sloth-runtime"),
        ])
        .status()
        .map_err(|e| format!("failed to invoke cargo: {e}"))?;
    if !status.success() {
        return Err(
            "failed to build sloth-runtime; run `cargo build -p sloth-runtime` manually"
                .to_string(),
        );
    }
    find_runtime_lib().map(|_| ())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Run { script, debug }) => {
            let source = read_script(&script);
            source.and_then(|source| {
                run_string_debug(&source, false, debug).map_err(|e| format!("{e:?}"))
            })
        }
        Some(Command::Build {
            script,
            output,
            emit,
            llc,
            cc,
            opt_level,
            target,
            runtime_lib,
            keep_temps,
        }) => {
            let emit_kind: EmitKind = emit.into();
            let output = output.unwrap_or_else(|| default_output(&script, emit_kind));
            read_script(&script).and_then(|source| {
                ensure_runtime_lib(emit_kind, &runtime_lib)?;
                let options = BuildOptions {
                    emit: emit_kind,
                    llc,
                    cc,
                    opt_level,
                    target,
                    runtime_lib,
                    keep_temps,
                };
                build_executable(&source, &output, &options)
            })
        }
        None => match cli.script {
            Some(script) => read_script(&script).and_then(|source| {
                if cli.debug {
                    run_string_debug(&source, false, true)
                } else {
                    run_string(&source, false)
                }
                .map_err(|e| format!("{e:?}"))
            }),
            None => Err("no script specified".to_string()),
        },
    };
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
