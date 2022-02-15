use crate::*;
//type NativeFResult = Result<Value, String>;
use std::collections::HashSet;

macro_rules! register_nfunc {
    ($([$f:ident,$s:expr]),*) => {
        pub fn native_map_parser() -> HashMap<String, usize> {
            let mut cnt = 0;
            let mut m = HashMap::new();
            $(m.insert($s.to_owned(), cnt);
            cnt += 1;)*
            m
        }
        pub fn native_map_vm() -> Vec<Value> {
            vec![$(Value::NativeFunction($f as *mut u8),)*]
        }
    };
}

register_nfunc!([sloth_print, "print"],[sloth_typeof,"typeof"]);

pub fn sloth_print(stack: &mut Vec<Value>, arg_num: usize, _protected: bool) -> Value {
    let mut visited_loc: HashSet<*mut u8> = HashSet::new();
    for _ in 0..arg_num {
        let val = stack.pop().unwrap_or(Value::Nil);
        print_val(&val, &mut visited_loc);
        println!();
    }
    Value::Nil
}
pub fn sloth_typeof(stack: &mut Vec<Value>, _arg_num: usize, _protected: bool) -> Value {
    let val = stack.pop().unwrap_or(Value::Nil);
    macro_rules! vstr {
        ($s:expr) => {
            Value::String($s.to_owned())
        };
    }
    match val {
        Value::Nil => vstr!("Nil"),
        Value::Bool(_) => vstr!("Bool"),
        Value::Number(_) => vstr!("Number"),
        Value::Vec2(_, _) => vstr!("Vec2"),
        Value::Vec3(_, _, _) => vstr!("Vec3"),
        Value::String(_) => vstr!("String"),
        Value::Array(_) => vstr!("Array"),
        Value::Dictionary(_) => vstr!("Dict"),
        Value::Error(_) => vstr!("Err"),
        Value::Matrix(_) => vstr!("Matrix"),
        Value::Closure(_) => vstr!("Closure"),
        Value::NativeFunction(_) => vstr!("NativeFunction"),
        Value::Chunk(_) => vstr!("Chunk"),
    }
}
fn print_val(val: &Value, visited_loc: &mut HashSet<*mut u8>) {
    match val {
        Value::Nil => {
            print!("Nil")
        }
        Value::Number(x) => {
            print!("{x}")
        }
        Value::Bool(b) => {
            print!("{b}")
        }
        Value::Vec2(x, y) => {
            print!("Vec2({x},{y})")
        }
        Value::Vec3(x, y, z) => {
            print!("Vec3({x},{y},{z})")
        }
        Value::String(s) => {
            print!("\"{s}\"")
        }
        Value::Array(a) => {
            if visited_loc.get(&(*a as *mut u8)).is_some() {
                print!("...");
            } else {
                visited_loc.insert(*a as *mut u8);
                print_array(*a, visited_loc);
                visited_loc.remove(&(*a as *mut u8));
            }
        }
        Value::Dictionary(d) => {
            if visited_loc.get(&(*d as *mut u8)).is_some() {
                print!("...");
            } else {
                visited_loc.insert(*d as *mut u8);
                print_dict(*d, visited_loc);
                visited_loc.remove(&(*d as *mut u8));
            }
        }
        Value::Error(d) => {
            if visited_loc.get(&(*d as *mut u8)).is_some() {
                print!("...");
            } else {
                visited_loc.insert(*d as *mut u8);
                print_dict(*d, visited_loc);
                visited_loc.remove(&(*d as *mut u8));
            }
        }
        Value::Matrix(m) => {
            print_mat(*m, visited_loc);
        }
        Value::Closure(p) => {
            print!("Closure@{p:?}")
        }
        Value::NativeFunction(p) => {
            print!("NativeFunc@{p:?}")
        }
        v => {
            print!("{v:?}")
        }
    }
}
fn print_array(arr: *mut Array, visited_loc: &mut HashSet<*mut u8>) {
    let arr = unsafe { &*arr };
    print!("[");
    for val in arr.array.iter() {
        print_val(val, visited_loc);
        print!(",");
    }
    print!("]");
}
fn print_dict(dict: *mut Dict, visited_loc: &mut HashSet<*mut u8>) {
    let dict = unsafe { &*dict };
    print!("{{");
    for (key, val) in dict.dict.iter() {
        print!("{key}>");
        print_val(val, visited_loc);
        print!(",");
    }
    print!("}}");
}
fn print_mat(mat: *mut Matrix, _visited_loc: &mut HashSet<*mut u8>) {
    let mat = unsafe { &*mat };
    print!("Matrix[");
    for i in 0..mat.row {
        print!("[");
        for j in 0..mat.col {
            print!("{},", mat.data[i * mat.col + j]);
        }
        print!("]");
    }
    print!("]");
}
