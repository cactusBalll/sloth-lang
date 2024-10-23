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

pub fn vec_u8_create(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let length = if let Value::Number(length) = vm.get_stack().pop().unwrap() {
        length
    } else {
        panic!("vec_u8_create take 1 parameter: length:Number.")
    };
    let _ = vm.get_stack().pop();
    let vec = vec![0u8; length as usize];
    let b_vec = Box::new(vec);
    // sloth guest program should manage it
    let p_vec = Box::into_raw(b_vec);
    vm.get_stack().push(Value::OpaqueData(p_vec as *mut u8));
}

pub fn vec_u8_set(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(3, arg_num);
    let val = if let Value::Number(val) = vm.get_stack().pop().unwrap() {
        val
    } else {
        panic!("vec_u8_set take 3 parameter: clct:OpaqueData, idx:Number, val:Number.")
    };
    let idx = if let Value::Number(idx) = vm.get_stack().pop().unwrap() {
        idx
    } else {
        panic!("vec_u8_set take 3 parameter: clct:OpaqueData, idx:Number, val:Number.")
    };
    let clct = if let Value::OpaqueData(clct) = vm.get_stack().pop().unwrap() {
        clct
    } else {
        panic!("vec_u8_set take 3 parameter: clct:OpaqueData, idx:Number, val:Number.")
    };
    let _ = vm.get_stack().pop();
    let clct = clct as *mut Vec<u8>;
    let idx = idx as usize;
    if val > 255. {
        panic!("val > 255 CANNOT set vec_u8");
    }
    unsafe {
        if idx >= (*clct).len() {
            panic!("index out of range, index: {} len: {}", idx, (*clct).len());
        }
        (*clct)[idx] = val as u8;
    }
    vm.get_stack().push(Value::Nil);
}

pub fn vec_u8_get(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(2, arg_num);
    let idx = if let Value::Number(idx) = vm.get_stack().pop().unwrap() {
        idx
    } else {
        panic!("vec_u8_get take 2 parameter: clct:OpaqueData, idx:Number.")
    };
    let clct = if let Value::OpaqueData(clct) = vm.get_stack().pop().unwrap() {
        clct
    } else {
        panic!("vec_u8_get take 2 parameter: clct:OpaqueData, idx:Number.")
    };
    let _ = vm.get_stack().pop();
    let clct = clct as *mut Vec<u8>;
    let idx = idx as usize;
    unsafe {
        if idx >= (*clct).len() {
            panic!("index out of range, index: {} len: {}", idx, (*clct).len());
        }
        let val = (*clct)[idx];
        vm.get_stack().push(Value::Number(val as f64));
    }
}


pub fn vec_u8_destroy(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(1, arg_num);
    let clct = if let Value::OpaqueData(clct) = vm.get_stack().pop().unwrap() {
        clct
    } else {
        panic!("vec_u8_destory take 1 parameter: clct: OpaqueData.")
    };
    let _ = vm.get_stack().pop();
    
    let clct = clct as *mut Vec<u8>;
    unsafe {
        // drop here
        let _box = Box::from_raw(clct);
    }
    vm.get_stack().push(Value::Nil);
}

pub fn module_export() -> (String, Vec<(String, Value)>) {
    let module_name = "vec_u8".to_owned();
    let module_func = vec![
        mf_entry!("create", vec_u8_create),
        mf_entry!("get", vec_u8_get),
        mf_entry!("set", vec_u8_set),
        mf_entry!("destory", vec_u8_destroy),
    ];

    (module_name, module_func)
}