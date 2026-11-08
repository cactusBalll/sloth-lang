#![cfg(feature = "nan-boxing")]

use std::path::PathBuf;
use std::process::Command;

use sloth_lang_core::backend::{build_executable, BuildOptions, EmitKind};

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn temp_dir(name: &str) -> PathBuf {
    let base = if cfg!(unix) && PathBuf::from("/tmp/opencode").is_dir() {
        PathBuf::from("/tmp/opencode")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("sloth-aot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn emits_llvm_for_small_program() {
    let dir = temp_dir("emit");
    let output = dir.join("prog.ll");
    let options = BuildOptions {
        emit: EmitKind::Llvm,
        ..Default::default()
    };
    let source = r#"
        func add(a, b) { return a + b; }
        print(add(20, 22));
    "#;
    build_executable(source, &output, &options).expect("IR generation failed");
    let ir = std::fs::read_to_string(&output).unwrap();
    assert!(ir.contains("@sloth_chunk_0"));
    assert!(ir.contains("@sloth_rt_new"));
    assert!(ir.contains("@sloth_image"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn llc_accepts_generated_ir() {
    if !tool_available("llc-21") {
        eprintln!("skipping: llc-21 is not available");
        return;
    }
    let dir = temp_dir("llc");
    let output = dir.join("prog.o");
    let options = BuildOptions {
        emit: EmitKind::Obj,
        ..Default::default()
    };
    let source = r#"
        var a = [1, 2, 3];
        for (var x: a) { print(x); }
    "#;
    build_executable(source, &output, &options).expect("llc rejected generated IR");
    assert!(output.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn end_to_end_import_and_fiber() {
    if !tool_available("llc-21") {
        eprintln!("skipping: llc-21 is not available");
        return;
    }
    if sloth_lang_core::backend::driver::find_runtime_lib().is_err() {
        eprintln!("skipping: libsloth_runtime.a is not built");
        return;
    }
    let dir = temp_dir("import");
    let output = dir.join("prog");
    let options = BuildOptions::default();
    let source = r#"
        var tools = import("sloth/sloth_lib/func_tool.slt");
        func add(a, b) { return a + b; }
        func mul2(a) { return a * 2; }
        var arr = [1, 2, 3];
        print(arr |> tools.map(mul2));
        print(arr |> tools.reduce(add, 0));
        var f = fiber.create(|n| {
            var i = 0;
            while (i < n) {
                fiber.yield(i);
                i = i + 1;
            }
        }, 3);
        print(fiber.resume(f));
        print(fiber.resume(f));
    "#;
    build_executable(source, &output, &options).expect("AOT build failed");
    let run = Command::new(&output)
        .output()
        .expect("failed to run AOT binary");
    assert!(
        run.status.success(),
        "AOT binary failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("[2,4,6,]"), "stdout: {stdout}");
    assert!(stdout.contains("6"), "stdout: {stdout}");
    assert!(stdout.contains('0'), "stdout: {stdout}");
    assert!(stdout.contains('1'), "stdout: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn aot_error_exits_nonzero() {
    if !tool_available("llc-21") {
        eprintln!("skipping: llc-21 is not available");
        return;
    }
    if sloth_lang_core::backend::driver::find_runtime_lib().is_err() {
        eprintln!("skipping: libsloth_runtime.a is not built");
        return;
    }
    let dir = temp_dir("error");
    let output = dir.join("prog");
    let options = BuildOptions::default();
    build_executable("print(1 / 0);", &output, &options).expect("AOT build failed");
    let run = Command::new(&output)
        .output()
        .expect("failed to run AOT binary");
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("div by 0"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn end_to_end_when_runtime_is_built() {
    if !tool_available("llc-21") {
        eprintln!("skipping: llc-21 is not available");
        return;
    }
    if sloth_lang_core::backend::driver::find_runtime_lib().is_err() {
        eprintln!("skipping: libsloth_runtime.a is not built");
        return;
    }
    let dir = temp_dir("exe");
    let output = dir.join("prog");
    let options = BuildOptions::default();
    let source = r#"
        func fib(n) {
            if (n < 2) { return n; }
            return fib(n - 1) + fib(n - 2);
        }
        print(fib(10));
        var a = [1, 2, 3];
        a[1] = 42;
        print(a);
        var f = |x| { return x * 2; };
        print(f(21));
    "#;
    build_executable(source, &output, &options).expect("AOT build failed");
    let run = Command::new(&output)
        .output()
        .expect("failed to run AOT binary");
    assert!(
        run.status.success(),
        "AOT binary failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("55"));
    assert!(stdout.contains("[1,42,3,]"));
    assert!(stdout.contains("42"));
    let _ = std::fs::remove_dir_all(&dir);
}
