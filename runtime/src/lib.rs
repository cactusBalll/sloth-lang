//! C ABI runtime for executables produced by the sloth AOT backend.
//!
//! The generated LLVM IR contains the program image and the per-chunk native
//! templates.  This library owns the VM state (value stack, call frames,
//! globals, upvalues, fibers, string pool and GC) and exposes small primitives
//! for the operations that the generated IR does not inline.

use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use sloth_lang_core::backend::image;
use sloth_lang_core::{fiber, math, prelude, vec, Instr, OpCode, Step, StringPool, Value, Vm};

/// Result codes shared with the generated IR.
const RT_CONTINUE: i32 = 0;
const RT_FRAME_CHANGED: i32 = 1;
const RT_FINISHED: i32 = 2;
const RT_ERROR: i32 = -1;

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic in sloth runtime".to_string()
    }
}

fn exec_catching(vm: &mut Vm, instr: Instr) -> i32 {
    let epoch = vm.frame_epoch();
    let result = catch_unwind(AssertUnwindSafe(|| vm.exec(instr)));
    match result {
        Ok(Ok(Step::Continue)) => {
            if vm.has_error() {
                RT_ERROR
            } else if vm.frame_epoch() != epoch {
                RT_FRAME_CHANGED
            } else {
                RT_CONTINUE
            }
        }
        Ok(Ok(Step::Finished)) => RT_FINISHED,
        Ok(Err(err)) => {
            vm.set_last_error(format!("{err}"));
            RT_ERROR
        }
        Err(payload) => {
            vm.set_last_error(panic_message(payload));
            RT_ERROR
        }
    }
}

fn instr_from_opcode(op: u32, operand: i64) -> Option<Instr> {
    use OpCode::*;
    let opcode = OpCode::from_u8(op as u8)?;
    let x = operand as usize;
    Some(match opcode {
        Nop => Instr::Nop,
        Load => Instr::Load(x),
        LoadChunk => Instr::LoadChunk(x),
        GetGlobal => Instr::GetGlobal(x),
        SetGlobal => Instr::SetGlobal(x),
        GetLocal => Instr::GetLocal(x),
        SetLocal => Instr::SetLocal(x),
        GetUpValue => Instr::GetUpValue(x),
        SetUpValue => Instr::SetUpValue(x),
        InitArray => Instr::InitArray(x),
        InitDict => Instr::InitDict(x),
        PushNil => Instr::PushNil,
        GetCollection => Instr::GetCollection,
        GetProperty => Instr::GetProperty,
        SetCollection => Instr::SetCollection(x),
        Add => Instr::Add,
        Sub => Instr::Sub,
        Mul => Instr::Mul,
        Div => Instr::Div,
        Mod => Instr::Mod,
        Negate => Instr::Negate,
        Not => Instr::Not,
        And => Instr::And,
        Or => Instr::Or,
        Gt => Instr::Gt,
        Lt => Instr::Lt,
        Ge => Instr::Ge,
        Le => Instr::Le,
        Eq => Instr::Eq,
        Ne => Instr::Ne,
        LoadTrue => Instr::LoadTrue,
        LoadFalse => Instr::LoadFalse,
        ClassIs => Instr::ClassIs,
        Pop => Instr::Pop,
        Swap2 => Instr::Swap2,
        Call => Instr::Call(x),
        JumpIfNot => Instr::JumpIfNot(operand as i32),
        JumpIfTrue => Instr::JumpIfTrue(operand as i32),
        Jump => Instr::Jump(operand as i32),
        Return => Instr::Return,
        MakeRange => Instr::MakeRange,
        MakeRangeClosed => Instr::MakeRangeClosed,
        Iterator => Instr::Iterator,
        Next => Instr::Next,
        InitClass => Instr::InitClass,
        AddMethod => Instr::AddMethod,
        ClassExtend => Instr::ClassExtend,
        GetSuperMethod => Instr::GetSuperMethod,
        GetThis => Instr::GetThis,
        UnpackVA => Instr::UnpackVA,
    })
}

/// Create a VM from a serialized program image.
#[no_mangle]
pub extern "C" fn sloth_rt_new(image_ptr: *const u8, image_len: usize) -> *mut Vm {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let bytes = unsafe { std::slice::from_raw_parts(image_ptr, image_len) };
        let mut pool = StringPool::new();
        let roots = image::deserialize(bytes, &mut pool).ok()?;
        let mut roots = roots.into_iter();
        let main = roots.next()?;
        let mut modules = HashMap::new();
        for root in roots {
            modules.insert(root.path, root.chunk);
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut vm = Vm::new(main.chunk, HashMap::new(), pool, false, cwd);
        vm.set_precompiled_modules(modules);
        vm.load_native_module(None, prelude());
        let (name, module) = fiber::module_export();
        vm.load_native_module(Some(&name), module);
        let (name, module) = vec::module_export();
        vm.load_native_module(Some(&name), module);
        let (name, module) = math::module_export();
        vm.load_native_module(Some(&name), module);
        Some(Box::into_raw(Box::new(vm)))
    }));
    match result {
        Ok(Some(vm)) => vm,
        Ok(None) => {
            eprintln!("sloth runtime: failed to initialize VM from program image");
            std::ptr::null_mut()
        }
        Err(payload) => {
            eprintln!(
                "sloth runtime: panic while initializing VM: {}",
                panic_message(payload)
            );
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_free(vm: *mut Vm) {
    if !vm.is_null() {
        unsafe {
            drop(Box::from_raw(vm));
        }
    }
}

/// Return the last error as a heap allocated C string.  The caller must free
/// the result with [`sloth_rt_free_string`].
#[no_mangle]
pub extern "C" fn sloth_rt_take_error(vm: *mut Vm) -> *mut c_char {
    if vm.is_null() {
        return std::ptr::null_mut();
    }
    let vm = unsafe { &mut *vm };
    match vm.take_error() {
        Some(message) => CString::new(message)
            .unwrap_or_else(|_| CString::new("error").unwrap())
            .into_raw(),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}

/// Flush Rust's stdout.  The generated `main` is a plain C entry point, so the
/// Rust runtime's normal shutdown flush does not run.
#[no_mangle]
pub extern "C" fn sloth_rt_flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Record an invalid program counter / chunk selection for diagnostics.
#[no_mangle]
pub extern "C" fn sloth_rt_bad_pc(vm: *mut Vm) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    let chunk = vm
        .current_chunk_id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let pc = vm.current_pc();
    vm.set_last_error(format!(
        "invalid program counter: chunk {chunk}, pc {pc} (not an instruction boundary)"
    ));
}

/// Print the pending error (if any) to stderr and clear it.
#[no_mangle]
pub extern "C" fn sloth_rt_report_error(vm: *mut Vm) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    match vm.take_error() {
        Some(message) => eprintln!("sloth runtime error: {message}"),
        None => eprintln!("sloth runtime error: unknown error"),
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_current_chunk(vm: *mut Vm) -> i64 {
    if vm.is_null() {
        return -1;
    }
    let vm = unsafe { &*vm };
    vm.current_chunk_id().map(|id| id as i64).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn sloth_rt_pc(vm: *mut Vm) -> i64 {
    if vm.is_null() {
        return 0;
    }
    unsafe { (*vm).current_pc() as i64 }
}

#[no_mangle]
pub extern "C" fn sloth_rt_set_pc(vm: *mut Vm, pc: i64) {
    if !vm.is_null() {
        unsafe { (*vm).set_pc(pc as usize) };
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_frame_epoch(vm: *mut Vm) -> u64 {
    if vm.is_null() {
        return 0;
    }
    unsafe { (*vm).frame_epoch() }
}

/// Execute one decoded instruction.  `operand` carries the instruction's only
/// operand; jumps pass their relative byte offset.
#[no_mangle]
pub extern "C" fn sloth_rt_exec(vm: *mut Vm, op: u32, operand: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    let Some(instr) = instr_from_opcode(op, operand) else {
        vm.set_last_error(format!("unknown opcode {op}"));
        return RT_ERROR;
    };
    exec_catching(vm, instr)
}

#[no_mangle]
pub extern "C" fn sloth_rt_push(vm: *mut Vm, bits: u64) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    vm.get_stack().push(unsafe { Value::from_bits(bits) });
}

/// Push an f64 result.  This boxes NaN, which cannot be stored verbatim in a
/// NaN-boxed value word.
#[no_mangle]
pub extern "C" fn sloth_rt_push_number(vm: *mut Vm, bits: u64) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    vm.get_stack().push(Value::Number(f64::from_bits(bits)));
}

#[no_mangle]
pub extern "C" fn sloth_rt_pop(vm: *mut Vm) -> u64 {
    if vm.is_null() {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &mut *vm };
    vm.get_stack().pop().unwrap_or(Value::Nil).into_bits()
}

/// Pop and drop the top value.
#[no_mangle]
pub extern "C" fn sloth_rt_pop_discard(vm: *mut Vm) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    let _ = vm.get_stack().pop();
}

/// Return the raw bits of the value at `depth` from the top without retaining
/// its payload.  Only valid for immediate inspection (tag checks).
#[no_mangle]
pub extern "C" fn sloth_rt_peek_raw(vm: *mut Vm, depth: i64) -> u64 {
    if vm.is_null() || depth < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &*vm };
    let stack = vm.get_stack();
    let depth = depth as usize;
    if depth >= stack.len() {
        Value::NIL_BITS
    } else {
        stack[stack.len() - 1 - depth].to_bits()
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_peek(vm: *mut Vm, depth: i64) -> u64 {
    if vm.is_null() || depth < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &*vm };
    let stack = vm.get_stack();
    let depth = depth as usize;
    if depth >= stack.len() {
        Value::NIL_BITS
    } else {
        stack[stack.len() - 1 - depth].clone().into_bits()
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_top_truthy(vm: *mut Vm) -> i32 {
    if vm.is_null() {
        return 0;
    }
    let vm = unsafe { &*vm };
    let stack = vm.get_stack();
    stack
        .last()
        .map(|value| value.to_bool() as i32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn sloth_rt_local_get(vm: *mut Vm, index: i64) -> u64 {
    if vm.is_null() || index < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &*vm };
    let bottom = vm.frame_bottom();
    let stack = vm.get_stack();
    let slot = bottom + index as usize;
    stack.get(slot).cloned().unwrap_or(Value::Nil).into_bits()
}

#[no_mangle]
pub extern "C" fn sloth_rt_local_set(vm: *mut Vm, index: i64, bits: u64) {
    if vm.is_null() || index < 0 {
        return;
    }
    let vm = unsafe { &mut *vm };
    let bottom = vm.frame_bottom();
    let stack = vm.get_stack();
    let slot = bottom + index as usize;
    if slot < stack.len() {
        stack[slot] = unsafe { Value::from_bits(bits) };
    } else {
        // The bytecode should never do this, but leaking one reference is safer
        // than corrupting the stack.
        unsafe { Value::release_bits(bits) };
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_swap2(vm: *mut Vm) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    let stack = vm.get_stack();
    let len = stack.len();
    if len >= 2 {
        stack.swap(len - 1, len - 2);
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_push_const(vm: *mut Vm, index: i64) {
    if vm.is_null() || index < 0 {
        return;
    }
    let vm = unsafe { &mut *vm };
    if let Some(value) = vm.current_chunk_constant(index as usize) {
        vm.get_stack().push(value);
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_retain(bits: u64) {
    unsafe { Value::retain_bits(bits) };
}

#[no_mangle]
pub extern "C" fn sloth_rt_release(bits: u64) {
    unsafe { Value::release_bits(bits) };
}

/// True when the current fiber has no call frames left.
#[no_mangle]
pub extern "C" fn sloth_rt_finished(vm: *mut Vm) -> i32 {
    if vm.is_null() {
        return 1;
    }
    let vm = unsafe { &*vm };
    if vm.current_chunk_id().is_none() {
        1
    } else {
        0
    }
}
