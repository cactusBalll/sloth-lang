use crate::*;
//type NativeFResult = Result<Value, String>;
use std::fmt::Write;
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
};


macro_rules! arity_assert {
    ($n:expr, $arg_num:expr) => {
        if $arg_num != $n {
            panic!("arity check failed, passed {}, required {}", $n, $arg_num);
        }
    };
}

macro_rules! mf_entry {
    ($name:expr,$func:expr) => {
        ($name.to_owned(), Value::NativeFunction($func as *mut u8))
    };
}

pub fn sloth_typeof(vm: &mut Vm, _arg_num: usize, _protected: bool) {
    let val = vm.get_stack().pop().unwrap_or(Value::Nil);
    macro_rules! vstr {
        ($s:expr) => {
            vm.make_managed_string($s)
        };
    }
    let v = match val {
        Value::Nil => vstr!("Nil"),
        Value::Bool(_) => vstr!("Bool"),
        Value::Number(_) => vstr!("Number"),
        Value::String(_) => vstr!("String"),
        Value::Array(_) => vstr!("Array"),
        Value::Module(_) => vstr!("Module"),
        Value::Dictionary(_) => vstr!("Dict"),
        Value::Error(_) => vstr!("Err"),
        Value::Closure(_) => vstr!("Closure"),
        Value::NativeFunction(_) => vstr!("NativeFunction"),
        Value::OpaqueData(_) => vstr!("OpaqueData"),
        Value::Fiber(_) => vstr!("Fiber"),
        Value::Range(_, _) => vstr!("Range"),
        _ => vstr!("..."),
    };
    vm.get_stack().push(Value::String(v));
}

pub fn sloth_add_glob(vm: &mut Vm, arg_num: usize, _protected: bool) {
    // add all kv to Global
    arity_assert!(1, arg_num);
    let p_dict = if let Value::Dictionary(p_dict) = vm.get_stack().pop().unwrap() {
        p_dict
    } else {
        panic!("add_glob take 1 argument: dict: Dict");
    };
    let _ = vm.get_stack().pop();
    unsafe {
        for (k,v) in (*p_dict).dict.iter() {
            vm.get_current_glob().insert(k.clone(), v.clone());
        }
    }
    vm.get_stack().push(Value::Nil);
}

pub fn sloth_load_module(vm: &mut Vm, arg_num: usize, _protected: bool) {
    if arg_num != 1 {
        vm.get_stack().pop();
        vm.get_stack().push(Value::Nil);
    }
    let path = vm.get_stack().pop().unwrap();
    if let Value::String(path) = path {
        vm.get_stack().pop();
        vm.fiber_changed = true;
        let full_path = vm.interpreter_cwd.join(path.get_inner());
        let mut file = File::open(&full_path).unwrap();
        let mut buf = String::new();
        file.read_to_string(&mut buf).unwrap();
        vm.load_module(&buf).unwrap();
        // return and entering load_module fiber
        // returned module will be pushed to stack later.
    } else {
        vm.get_stack().pop();
        vm.get_stack().push(Value::Nil);
    }
}

pub fn sloth_print_val(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let mut val_to_print = Vec::new();
    for _ in 0..arg_num {
        val_to_print.push(vm.get_stack().pop().unwrap());
    }
    // pop me
    let _ = vm.get_stack().pop();
    val_to_print.reverse();
    for v in val_to_print {
        let mut vis = HashSet::new();
        print_val(&v, &mut vis);
        print!(" ");
    }
    // Functions always have ONE return Value
    vm.get_stack().push(Value::Nil);
}

pub fn sloth_input(vm: &mut Vm, _arg_num: usize, _protected: bool) {
    let mut buffer = String::new();
    // blocking...
    let _ = io::stdin().read_line(&mut buffer);
    let istring = vm.make_managed_string(buffer.trim());
    vm.get_stack().push(Value::String(istring));
}

pub fn sloth_to_number(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let val = vm.get_stack().pop().unwrap();
    let _ = vm.get_stack().pop();

    let num = match val {
        Value::Bool(b) => {
            if b {
                1.
            } else {
                0.
            }
        }
        Value::String(s) => {
            let s1 = s.get_inner();
            if let Ok(num) = s1.parse::<f64>() {
                num
            } else {
                vm.get_stack().push(Value::Nil);
                return;
            }
        }
        _ => {
            vm.get_stack().push(Value::Nil);
            return;
        }
    };

    vm.get_stack().push(Value::Number(num));
}

pub fn sloth_to_bool(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let val = vm.get_stack().pop().unwrap();
    let _ = vm.get_stack().pop();
    vm.get_stack().push(val.to_bool_v());
}

pub fn sloth_to_string(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let val = vm.get_stack().pop().unwrap();
    let _ = vm.get_stack().pop();
    let mut vis = HashSet::new();
    let mut buffer = String::new();
    if let Ok(_) = write_val(&mut buffer, &val, &mut vis) {
        let s = vm.make_managed_string(&buffer);
        vm.get_stack().push(Value::String(s));
    } else {
        vm.get_stack().push(Value::Nil);
    }
}

pub fn sloth_va_arg(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let _ = vm.get_stack().pop();
    let va_arg_vec = vm.get_call_frame().va_args.clone();
    let mut b_array = Box::new(Array{marked: false, array: va_arg_vec});
    let p_array = b_array.as_mut() as *mut Array;
    vm.add_object(b_array);

    vm.get_stack().push(Value::Array(p_array));
}


pub fn sloth_ord(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let s = if let Value::String(s) = vm.get_stack().pop().unwrap() {
        s
    } else {
        panic!("ord take 1 argument: s: String.");
    };
    let _ = vm.get_stack().pop();

    if s.get_inner().len() != 1{
        panic!("ord(s): s should contains exactly **1** char.")
    }
    let val = s.get_inner().chars().next().unwrap();
    vm.get_stack().push(Value::Number(val as usize as f64));
}

pub fn sloth_chr(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let v = if let Value::Number(v) = vm.get_stack().pop().unwrap() {
        v
    } else {
        panic!("chr take 1 argument: v: Number.");
    };
    let _ = vm.get_stack().pop();

    if let Some(c) = std::char::from_u32(v as u32) {
        let mut s = String::new();
        s.push(c);
        let s = vm.make_managed_string(&s);
        vm.get_stack().push(Value::String(s));
    } else {
        panic!("chr(): not a valid unicode codepoint.");
    }
}
fn write_val(buffer: &mut String, val: &Value, visited_loc: &mut HashSet<*mut u8>) -> fmt::Result {
    match val {
        Value::Nil => {
            write!(buffer, "Nil")?;
        }
        Value::Number(x) => {
            write!(buffer, "{x}")?;
        }
        Value::Bool(b) => {
            write!(buffer, "{b}")?;
        }

        Value::String(s) => {
            write!(buffer, "{s}")?;
        }
        Value::Array(a) => {
            if visited_loc.get(&(*a as *mut u8)).is_some() {
                write!(buffer, "...")?;
            } else {
                visited_loc.insert(*a as *mut u8);
                write_array(buffer, *a, visited_loc)?;
                visited_loc.remove(&(*a as *mut u8));
            }
        }
        Value::Dictionary(d) => {
            if visited_loc.get(&(*d as *mut u8)).is_some() {
                write!(buffer, "...")?;
            } else {
                visited_loc.insert(*d as *mut u8);
                write_dict(buffer, *d, visited_loc)?;
                visited_loc.remove(&(*d as *mut u8));
            }
        }
        Value::Error(d) => {
            if visited_loc.get(&(*d as *mut u8)).is_some() {
                write!(buffer, "...")?;
            } else {
                visited_loc.insert(*d as *mut u8);
                write_dict(buffer, *d, visited_loc)?;
                visited_loc.remove(&(*d as *mut u8));
            }
        }
        Value::Closure(p) => {
            write!(buffer, "Closure@{p:?}")?;
        }
        Value::NativeFunction(p) => {
            write!(buffer, "NativeFunc@{p:?}")?;
        }
        v => {
            write!(buffer, "{v:?}")?;
        }
    }
    Ok(())
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

        Value::String(s) => {
            print!("{s}")
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

fn write_array(
    buffer: &mut String,
    arr: *mut Array,
    visited_loc: &mut HashSet<*mut u8>,
) -> fmt::Result {
    let arr = unsafe { &*arr };
    write!(buffer, "[")?;
    for val in arr.array.iter() {
        write_val(buffer, val, visited_loc)?;
        write!(buffer, ",")?;
    }
    write!(buffer, "]")?;
    Ok(())
}
fn print_dict(dict: *mut Dict, visited_loc: &mut HashSet<*mut u8>) {
    let dict = unsafe { &*dict };
    print!("@(");
    for (key, val) in dict.dict.iter() {
        print!("{key}:");
        print_val(val, visited_loc);
        print!(",");
    }
    print!(")");
}

fn write_dict(
    buffer: &mut String,
    dict: *mut Dict,
    visited_loc: &mut HashSet<*mut u8>,
) -> fmt::Result {
    let dict = unsafe { &*dict };
    write!(buffer, "@(")?;
    for (key, val) in dict.dict.iter() {
        write!(buffer, "{key}:")?;
        write_val(buffer, val, visited_loc)?;
        write!(buffer, ",")?;
    }
    write!(buffer, ")")?;
    Ok(())
}
