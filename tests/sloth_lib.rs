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
fn test1() {
    let src = r#"
        var a = 3;
        if (a > 4) {
            print("yes");
        }
        if (a >= 3) {
            print("yes");
        }
    "#;
    let res = run_string_debug(&src, false, false);
    println!("{res:?}");
}

#[test]
fn brainfk() {
    run_file("sloth/sloth_examples/brainfk.slt".into(), false, false);
}
#[test]
fn game_of_life() {
    run_file("sloth/sloth_examples/game_of_life.slt".into(), false, false);
}
#[test]
fn snake() {
    run_file("sloth/sloth_examples/snake.slt".into(), false, false);
}

fn run_file(path: PathBuf, only_compile: bool, debug: bool) {
    let cwd = std::env::current_dir().unwrap();
    let mut full_path = PathBuf::new();
    full_path.push(&cwd);
    full_path.push(&path);
    let mut file = std::fs::File::open(full_path).unwrap();
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).unwrap();
    let res = run_string_debug(&buffer, only_compile, debug);
    if let Err(e) = res {
        eprintln!("{e:?}");
    }
}
