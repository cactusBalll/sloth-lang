use crate::{vm::Vm, Value};

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

pub fn array_push(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let v = vm.get_stack().pop().unwrap();
    let _ = vm.get_stack().pop();
    // Array is hided under me
    let clct = vm.get_stack().pop().unwrap();

    if let Value::Array(p_arr) = clct {
        unsafe {
            (*p_arr).array.push(v);
        }
    } else {
        panic!("`array_push` can ONLY push to Array.");
    }

    vm.get_stack().push(Value::Nil);
}

pub fn array_pop(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(0, arg_num);
    let _ = vm.get_stack().pop();
    // Array is hided under me
    let clct = vm.get_stack().pop().unwrap();

    if let Value::Array(p_arr) = clct {
        unsafe {
            let v = (*p_arr).array.pop().unwrap();
            vm.get_stack().push(v);
        }
    } else {
        panic!("`array_push` can ONLY push to Array.");
    }
}

pub fn array_pop_front(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(0, arg_num);
    let _ = vm.get_stack().pop();
    // Array is hided under me
    let clct = vm.get_stack().pop().unwrap();

    if let Value::Array(p_arr) = clct {
        unsafe {
            let v = (*p_arr).array.remove(0);
            vm.get_stack().push(v);
        }
    } else {
        panic!("`array_pop_front` can ONLY pop Array.");
    }
}

pub fn array_remove(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let idx = vm.gets_number();
    let _ = vm.get_stack().pop();
    // Array is hided under me
    let clct = vm.get_stack().pop().unwrap();

    if let Value::Array(p_arr) = clct {
        unsafe {
            let v = (*p_arr).array.remove(idx as usize);
            vm.get_stack().push(v);
        }
    } else {
        panic!("`array_pop_front` can ONLY remove from Array.");
    }
}

pub fn array_insert(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(2, arg_num);
    let v = vm.get_stack().pop().unwrap();
    let idx = vm.gets_number();
    let _ = vm.get_stack().pop();
    // Array is hided under me
    let clct = vm.get_stack().pop().unwrap();

    if let Value::Array(p_arr) = clct {
        unsafe {
            (*p_arr).array.insert(idx as usize, v);
        }
    } else {
        panic!("`array_inser` can ONLY insert into Array.");
    }
    vm.get_stack().push(Value::Nil);
}

