extern crate sloth_lang_core;
use sloth_lang_core::run_string;

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
