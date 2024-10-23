use std::ptr::null_mut;

use crate::{
    vm::{CallFrame, Vm},
    Fiber, FiberState, Value,
};

macro_rules! arity_assert {
    ($n:expr, $arg_num:expr) => {
        if $arg_num != $n {
            panic!("arity check failed, passed {}, required {}", $n, $arg_num);
        }
    };
}
// initial -> waiting -> paused -> 
pub fn sloth_fiber_create(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let mut args = Vec::new();
    let arg_cnt = arg_num - 1;
    for _ in 0..arg_cnt {
        args.push(vm.get_stack().pop().unwrap());
    }
    args.reverse();
    let closure = vm.get_stack().pop().unwrap();
    let _ = vm.get_stack().pop();
    let p_closure = if let Value::Closure(p_closure) = closure {
        p_closure
    } else {
        panic!("creating Fiber with something not callable.");
    };

    let mut stack = Vec::new();
    // bind arguments
    let mut packed_va_list = Vec::new();
    let chunk = unsafe { &*(*p_closure).chunk };
    if chunk.parameter_num != arg_cnt {
        if chunk.parameter_num < arg_cnt && chunk.is_va {
            for idx in chunk.parameter_num..args.len() {
                packed_va_list.push(args[idx].clone());
            }
        } else {
            panic!("cannot create fiber with closure and incorrect arglist.");
        }
    }
    // dbg!(chunk.parameter_num);
    // dbg!(chunk.num_locals);
    for idx in 0..chunk.parameter_num {
        stack.push(args[idx].clone());
    }
    for _ in 0..(chunk.num_locals - chunk.parameter_num) {
        stack.push(Value::Nil);
    }
    // initialize fiber
    let mut b_fiber = Box::new(Fiber {
        marked: false,
        call_frames: vec![CallFrame::new(0, p_closure, packed_va_list)],
        stack,
        state: crate::FiberState::Initial,
        prev: null_mut() as *mut Fiber,
    });
    let p_fiber = b_fiber.as_mut() as *mut Fiber;
    vm.add_object(b_fiber);
    vm.get_stack().push(Value::Fiber(p_fiber));
}

pub fn sloth_fiber_resume(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let pass_val = if arg_num == 2 {
        vm.get_stack().pop().unwrap()
    } else {
        Value::Nil
    };
    let fiber = if let Value::Fiber(f) = vm.get_stack().pop().unwrap() {
        f
    } else {
        panic!("can only resume Fiber");
    };
    let _ = vm.get_stack().pop();
    unsafe {
        if (*fiber).state != FiberState::Paused && (*fiber).state != FiberState::Initial{
            panic!("ONLY Fiber in Paused Or Initial State can be resume.");
        }
        (*vm.get_current_fiber()).state = FiberState::Waiting;
        if (*fiber).state == FiberState::Paused {
            (*fiber).stack.push(pass_val);
        }
        (*fiber).prev = vm.get_current_fiber();
        if (*fiber).state == FiberState::Initial {
            vm.fiber_changed = true;
        }
        (*fiber).state = FiberState::Running;
        vm.set_fiber(fiber);
        
    }
}

pub fn sloth_fiber_yield(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let pass_val = if arg_num == 1 {
        vm.get_stack().pop().unwrap()
    } else {
        Value::Nil
    };
    let _ = vm.get_stack().pop();
    unsafe {
        (*vm.get_current_fiber()).state = FiberState::Paused;
        let prev = (*vm.get_current_fiber()).prev;
        if prev == null_mut() {
            panic!("yield to nowhere");
        }
        (*prev).stack.push(pass_val);
        (*prev).state = FiberState::Running;
        vm.set_fiber(prev);

    }
}

pub fn sloth_fiber_transfer(vm: &mut Vm, arg_num: usize, _protected: bool) {
    arity_assert!(0, arg_num);
    let fiber = if let Value::Fiber(f) = vm.get_stack().pop().unwrap() {
        f
    } else {
        panic!("can only resume Fiber");
    };
    let _ = vm.get_stack().pop();
    unsafe {
        if (*fiber).state != FiberState::Paused && (*fiber).state != FiberState::Initial{
            panic!("ONLY Fiber in Paused Or Initial State can be transfered to.");
        }
        (*vm.get_current_fiber()).state = FiberState::Paused;
        if (*fiber).state == FiberState::Paused {
            (*fiber).stack.push(Value::Nil);
        }
        if (*fiber).state == FiberState::Initial {
            vm.fiber_changed = true;
        }
        (*fiber).state = FiberState::Running;
        vm.set_fiber(fiber);
        
    }

}
pub fn sloth_fiber_set_error(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let _ = vm.get_stack().pop();
    unsafe {
        (*vm.get_current_fiber()).state = FiberState::Error;
        let prev = (*vm.get_current_fiber()).prev;
        if prev == null_mut() {
            panic!("fiber error occured but nowhere to go.");
        }
        (*prev).state = FiberState::Running;
        vm.set_fiber(prev);
        vm.get_stack().push(Value::Nil);
    }
}

pub fn sloth_fiber_check(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let fiber = if let Value::Fiber(f) = vm.get_stack().pop().unwrap() {
        f
    } else {
        panic!("not a Fiber");
    };
    let _ = vm.get_stack().pop();
    unsafe {
        let ok = (*fiber).state != FiberState::Error;
        vm.get_stack().push(Value::Bool(ok));
    }
}

pub fn sloth_fiber_resumable(vm: &mut Vm, arg_num: usize, _protected: bool) {
    let fiber = if let Value::Fiber(f) = vm.get_stack().pop().unwrap() {
        f
    } else {
        panic!("not a Fiber");
    };
    let _ = vm.get_stack().pop();
    unsafe {
        let ok = (*fiber).state == FiberState::Paused || (*fiber).state == FiberState::Initial;
        vm.get_stack().push(Value::Bool(ok));
    }
}
macro_rules! mf_entry {
    ($name:expr,$func:expr) => {
        ($name.to_owned(), Value::NativeFunction($func as *mut u8))
    };
}
pub fn module_export() -> (String, Vec<(String, Value)>) {
    let module_name = "fiber".to_owned();
    let module_func = vec![
        mf_entry!("create", sloth_fiber_create),
        mf_entry!("resume", sloth_fiber_resume),
        mf_entry!("yield", sloth_fiber_yield),
        mf_entry!("error", sloth_fiber_set_error),
        mf_entry!("check", sloth_fiber_check),
        mf_entry!("resumable", sloth_fiber_resumable),
        mf_entry!("transfer", sloth_fiber_transfer),
    ];

    (module_name, module_func)
}
