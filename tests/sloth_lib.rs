extern crate sloth_lang_core;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use sloth_lang_core::{run_string, run_string_debug};

#[test]
fn func_tool() {
    let src = r#"
        var func_tools = import("sloth/sloth_lib/func_tool.slt");
        func add(a,b) {
            return a + b;
        }
        func mul2(a) {
            return a * 2;
        }
        var reduce = func_tools.reduce;
        var map = func_tools.map;
        var arr = [1,2,3,4,5];
        print(arr |> reduce(add, 0), arr |> map(mul2));
    "#;
    let res = run_string(&src, false);
    println!("{res:?}");
}

#[test]
fn brainfk() {
    run_file("sloth/sloth_examples/brainfk.slt".into(), false);
}

fn run_file(path: PathBuf, debug: bool) {
    let cwd = std::env::current_dir().unwrap();
    let mut full_path = PathBuf::new();
    full_path.push(&cwd);
    full_path.push(&path);
    let mut file = std::fs::File::open(full_path).unwrap();
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).unwrap();
    let res = run_string_debug(&buffer, false, debug);
    if debug {
        eprintln!("{res:?}");
    }
}
