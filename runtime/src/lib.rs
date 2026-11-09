//! C ABI runtime for executables produced by the sloth AOT backend.
//!
//! The generated LLVM IR contains the program image, the per-chunk native
//! templates and direct accesses to the value stack.  This library owns the VM
//! state (value stack, call frames, globals, upvalues, fibers, string pool and
//! GC) and exposes one small entry point per non-inlined operation.

use std::collections::HashMap;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use sloth_lang_core::backend::image;
use sloth_lang_core::{fiber, math, prelude, vec, Instr, Step, StringPool, Value, Vm};

/// Result codes shared with the generated IR.
const RT_CONTINUE: i32 = 0;
const RT_FRAME_CHANGED: i32 = 1;
const RT_FINISHED: i32 = 2;
const RT_ERROR: i32 = -1;

// ===========================================================================
// Direct value-stack access
// ===========================================================================
//
// The generated code uses these globals to push/pop values and to address
// locals without calling into the runtime for every stack operation.

#[no_mangle]
pub static mut sloth_rt_stack_data: *mut u64 = std::ptr::null_mut();
#[no_mangle]
pub static mut sloth_rt_stack_len: usize = 0;
#[no_mangle]
pub static mut sloth_rt_stack_cap: usize = 0;
#[no_mangle]
pub static mut sloth_rt_frame_base: usize = 0;

/// Publish the VM's current stack state to the generated code.
#[inline]
unsafe fn sync_out(vm: &Vm) {
    let stack = vm.get_stack();
    sloth_rt_stack_data = stack.as_ptr() as *mut u64;
    sloth_rt_stack_len = stack.len();
    sloth_rt_stack_cap = stack.capacity();
    sloth_rt_frame_base = vm.frame_bottom();
}

/// Apply the stack length maintained by the generated code to the VM's `Vec`.
/// The generated code always initialises a slot before publishing a larger
/// length, so `set_len` never exposes uninitialised values.
#[inline]
unsafe fn sync_in(vm: &mut Vm) {
    let len = sloth_rt_stack_len;
    let stack = vm.get_stack();
    if len > stack.capacity() {
        stack.reserve(len.saturating_sub(stack.len()));
    }
    stack.set_len(len);
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic in sloth runtime".to_string()
    }
}

fn report_step(
    vm: &mut Vm,
    epoch: u64,
    result: Result<Result<Step, sloth_lang_core::EvalError>, Box<dyn std::any::Any + Send>>,
) -> i32 {
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

/// Run one dedicated slow-path operation, catching panics at the FFI boundary
/// and re-synchronising the stack afterwards.
fn run_op<F>(vm: &mut Vm, pc: Option<usize>, operation: F) -> i32
where
    F: FnOnce(&mut Vm) -> Result<Step, sloth_lang_core::EvalError>,
{
    let epoch = vm.frame_epoch();
    if let Some(pc) = pc {
        vm.set_pc(pc);
    }
    unsafe {
        sync_in(vm);
    }
    let result = catch_unwind(AssertUnwindSafe(|| operation(vm)));
    unsafe {
        sync_out(vm);
    }
    report_step(vm, epoch, result)
}

fn run_simple<F>(vm: &mut Vm, pc: Option<usize>, operation: F) -> i32
where
    F: FnOnce(&mut Vm) -> Result<(), sloth_lang_core::EvalError>,
{
    run_op(vm, pc, |vm| {
        operation(vm)?;
        Ok(Step::Continue)
    })
}

// ===========================================================================
// VM lifecycle
// ===========================================================================

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
        unsafe {
            sync_out(&vm);
        }
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

// ===========================================================================
// Stack helpers used by generated prologues and slow paths
// ===========================================================================

/// Ensure that at least `extra` further values fit on the value stack.
#[no_mangle]
pub extern "C" fn sloth_rt_stack_ensure(vm: *mut Vm, extra: i64) {
    if vm.is_null() {
        return;
    }
    let vm = unsafe { &mut *vm };
    unsafe {
        sync_in(vm);
        let extra = extra.max(0) as usize;
        vm.get_stack().reserve(extra);
        sync_out(vm);
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

// ===========================================================================
// Constants, globals, upvalues and `this`
// ===========================================================================

#[no_mangle]
pub extern "C" fn sloth_rt_get_const(vm: *mut Vm, index: i64) -> u64 {
    if vm.is_null() || index < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &mut *vm };
    vm.current_chunk_constant(index as usize)
        .unwrap_or(Value::Nil)
        .into_bits()
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_global(vm: *mut Vm, index: i64) -> u64 {
    if vm.is_null() || index < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &mut *vm };
    let result = catch_unwind(AssertUnwindSafe(|| vm.get_global_value(index as usize)));
    match result {
        Ok(value) => value.into_bits(),
        Err(payload) => {
            vm.set_last_error(panic_message(payload));
            Value::NIL_BITS
        }
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_set_global(vm: *mut Vm, index: i64, bits: u64) {
    if vm.is_null() || index < 0 {
        unsafe { Value::release_bits(bits) };
        return;
    }
    let vm = unsafe { &mut *vm };
    let value = unsafe { Value::from_bits(bits) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        vm.set_global_value(index as usize, value)
    }));
    if let Err(payload) = result {
        vm.set_last_error(panic_message(payload));
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_upvalue(vm: *mut Vm, index: i64) -> u64 {
    if vm.is_null() || index < 0 {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &mut *vm };
    let result = catch_unwind(AssertUnwindSafe(|| vm.get_upvalue(index as usize)));
    match result {
        Ok(value) => value.into_bits(),
        Err(payload) => {
            vm.set_last_error(panic_message(payload));
            Value::NIL_BITS
        }
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_set_upvalue(vm: *mut Vm, index: i64, bits: u64) {
    if vm.is_null() || index < 0 {
        unsafe { Value::release_bits(bits) };
        return;
    }
    let vm = unsafe { &mut *vm };
    let value = unsafe { Value::from_bits(bits) };
    let result = catch_unwind(AssertUnwindSafe(|| vm.set_upvalue(index as usize, value)));
    if let Err(payload) = result {
        vm.set_last_error(panic_message(payload));
    }
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_this(vm: *mut Vm) -> u64 {
    if vm.is_null() {
        return Value::NIL_BITS;
    }
    let vm = unsafe { &mut *vm };
    let result = catch_unwind(AssertUnwindSafe(|| vm.get_this_value()));
    match result {
        Ok(value) => value.into_bits(),
        Err(payload) => {
            vm.set_last_error(panic_message(payload));
            Value::NIL_BITS
        }
    }
}

// ===========================================================================
// Dedicated slow paths
// ===========================================================================
//
// The generated code sets the VM pc to the current instruction before calling
// an ABI that may report an error.  `next_pc` is the statically known offset
// of the following instruction, so the shared VM operation can advance the
// frame without decoding the bytecode again.

#[no_mangle]
pub extern "C" fn sloth_rt_load_chunk(vm: *mut Vm, index: i64, next_pc: i64) -> i32 {
    if vm.is_null() || index < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.load_chunk(index as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_unpack_va(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.unpack_va(Some(next_pc as usize));
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_init_array(vm: *mut Vm, count: i64, next_pc: i64) -> i32 {
    if vm.is_null() || count < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.init_array(count as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_init_dict(vm: *mut Vm, count: i64, next_pc: i64) -> i32 {
    if vm.is_null() || count < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.init_dict(count as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_iterator(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.make_iterator(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_next(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.iterator_next(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_make_range(vm: *mut Vm, closed: i64, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.make_range(closed != 0, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_class_is(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.class_is(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_init_class(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.init_class(Some(next_pc as usize));
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_class_extend(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.class_extend(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_add_method(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.add_method(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_super_method(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.get_super_method(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_collection(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.exec_get_collection(Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_get_property(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.exec_get_property(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_set_collection(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.exec_set_collection(Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_set_property(vm: *mut Vm, next_pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| vm.exec_set_property(Some(next_pc as usize)))
}

#[no_mangle]
pub extern "C" fn sloth_rt_invoke_property(vm: *mut Vm, argc: i64, next_pc: i64) -> i32 {
    if vm.is_null() || argc < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.exec_invoke_property(argc as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_invoke_super(vm: *mut Vm, argc: i64, next_pc: i64) -> i32 {
    if vm.is_null() || argc < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.exec_invoke_super(argc as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_call(vm: *mut Vm, argc: i64, next_pc: i64) -> i32 {
    if vm.is_null() || argc < 0 {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_simple(vm, None, |vm| {
        vm.call_value(argc as usize, Some(next_pc as usize))
    })
}

#[no_mangle]
pub extern "C" fn sloth_rt_return(vm: *mut Vm) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let vm = unsafe { &mut *vm };
    run_op(vm, None, |vm| vm.do_return())
}

/// Slow path for binary operators whose operands are not both numbers (string
/// concatenation, array concatenation, operator overloads and type errors).
#[no_mangle]
pub extern "C" fn sloth_rt_binary_slow(vm: *mut Vm, kind: i64, pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let instr = match kind {
        0 => Instr::Add,
        1 => Instr::Sub,
        2 => Instr::Mul,
        3 => Instr::Div,
        4 => Instr::Mod,
        5 => Instr::And,
        6 => Instr::Or,
        7 => Instr::Gt,
        8 => Instr::Lt,
        9 => Instr::Ge,
        10 => Instr::Le,
        11 => Instr::Eq,
        12 => Instr::Ne,
        _ => return RT_ERROR,
    };
    let vm = unsafe { &mut *vm };
    run_op(vm, Some(pc as usize), |vm| vm.exec(instr))
}

/// Slow path for unary operators whose operand is not an immediate/number.
#[no_mangle]
pub extern "C" fn sloth_rt_unary_slow(vm: *mut Vm, kind: i64, pc: i64) -> i32 {
    if vm.is_null() {
        return RT_ERROR;
    }
    let instr = match kind {
        0 => Instr::Negate,
        1 => Instr::Not,
        _ => return RT_ERROR,
    };
    let vm = unsafe { &mut *vm };
    run_op(vm, Some(pc as usize), |vm| vm.exec(instr))
}
