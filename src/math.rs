use crate::{interned_string::IString, vm::Vm, Value};
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

pub fn sloth_math_floor(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let v = vm.gets_number();
    let _ = vm.get_stack().pop();
    vm.get_stack().push(Value::Number(v.floor()));
}

pub fn sloth_math_ceil(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let v = vm.gets_number();
    let _ = vm.get_stack().pop();
    vm.get_stack().push(Value::Number(v.ceil()));
}

pub fn module_export() -> (String, Vec<(String, Value)>) {
    let module_name = "math".to_owned();

    let module_func = vec![
        mf_entry!("floor", sloth_math_floor),
        mf_entry!("ceil", sloth_math_ceil),  
    ];
    return (module_name, module_func);
}
