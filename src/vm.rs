use fmt::format;
use interned_string::{IString, StringPool};

use crate::*;
use std::{
    collections::btree_map::Range,
    fmt::Display,
    path::PathBuf,
    ptr::{self, null_mut},
};
#[derive(Debug)]
pub enum EvalError {
    Error(String),
    ArithmError(String),
    TypeError(String),
    IndexOutOfBound(String),
    CallError(String),
    VariableNotFound(String),
    KeyError(String),
    GCError,
}
impl Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl From<EvalError> for String {
    fn from(err: EvalError) -> String {
        err.to_string()
    }
}

impl From<String> for EvalError {
    fn from(value: String) -> Self {
        EvalError::Error(value)
    }
}
type EvalResult = Result<(), EvalError>;

/// Result of executing a single instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The instruction completed and execution should continue.
    Continue,
    /// The top-level fiber has no more call frames; the program is finished.
    Finished,
}

pub struct Vm {
    executing_fiber: *mut Fiber,
    upvalues: Vec<*mut UpValueObject>,
    objects: Vec<Box<dyn GCObject>>,
    // chunks: Vec<Chunk>,
    top_chunk: Box<Chunk>,     // no gc during running
    top_closure: Box<Closure>, // no gc during running
    main_fiber: Box<Fiber>,    // no gc during running
    /// should load other modules in seperated global namespace
    global: Vec<HashMap<IString, Value>>,

    /// modules other than ther main module
    loaded_chunk: Vec<Box<Chunk>>,

    string_pool: StringPool,
    debug: bool,

    pub interpreter_cwd: PathBuf,

    /// Incremented whenever a call frame is pushed/popped or the executing
    /// fiber changes.  The AOT dispatcher uses it to decide when it has to
    /// resynchronize with the VM.
    frame_epoch: u64,
    /// Modules precompiled by the AOT backend, keyed by their import path.
    precompiled_modules: Option<HashMap<String, Chunk>>,
    /// Error captured at the C ABI boundary.
    last_error: Option<String>,
}

/// every fiber have its own stack

#[derive(Debug)]
pub struct CallFrame {
    bottom: usize,
    pub closure: *mut Closure,
    pub pc: usize,

    pub va_args: Vec<Value>,

    /// Receiver bound by a direct method invocation (`obj.method(...)`).
    /// Takes precedence over `closure.this_ref`, which is used by lambdas
    /// created inside methods.
    pub bound_this: Option<*mut Instance>,

    pub discard_return_value: bool,
}
impl CallFrame {
    pub fn new(bottom: usize, closure: *mut Closure, va_args: Vec<Value>) -> CallFrame {
        CallFrame {
            bottom,
            closure,
            pc: 0,
            va_args,
            bound_this: None,
            discard_return_value: false,
        }
    }
    fn decode(&self) -> Result<(Instr, usize), u8> {
        unsafe {
            let chunk = &*(*self.closure).chunk;
            chunk.decode(self.pc)
        }
    }
    fn nxt(&mut self) {
        let len = match self.decode() {
            Ok((_, len)) => len,
            // Let the main loop report the malformed instruction.  Advancing by
            // one keeps `pc_add` from looping forever.
            Err(_) => 1,
        };
        self.pc += len;
    }
    fn jump(&mut self, offset: i32) {
        self.pc = (self.pc as i32 + offset) as usize;
    }
}
impl Vm {
    pub fn new(
        prog: Chunk,
        global: HashMap<IString, Value>,
        string_pool: StringPool,
        debug: bool,
        interpreter_cwd: PathBuf,
    ) -> Vm {
        let mut call_frames = Vec::<CallFrame>::new();
        let mut b_chunk = Box::new(prog);
        let mut closure = Box::new(Closure {
            marked: false,
            chunk: b_chunk.as_mut() as *const Chunk,
            upvalues: Vec::new(),
            this_ref: None,
        });
        call_frames.push(CallFrame::new(
            0,
            closure.as_mut() as *mut Closure,
            Vec::new(),
        ));
        let mut fiber = Box::new(Fiber {
            marked: false,
            call_frames: call_frames,
            stack: {
                let mut vec = Vec::new();
                for _ in 0..b_chunk.num_locals {
                    vec.push(Value::Nil);
                }
                vec
            },
            // main fiber is always waiting, which prevent other fibers from resuming it
            state: FiberState::Running,
            prev: null_mut() as *mut Fiber,
        });

        Vm {
            executing_fiber: fiber.as_mut() as *mut Fiber,
            upvalues: Vec::new(),
            objects: Vec::new(),
            top_closure: closure,
            top_chunk: b_chunk,
            main_fiber: fiber,
            global: vec![global],
            loaded_chunk: Vec::new(),
            string_pool,
            debug,
            interpreter_cwd,
            frame_epoch: 0,
            precompiled_modules: None,
            last_error: None,
        }
    }
    pub fn get_stack<'a>(&'a self) -> &'a mut Vec<Value> {
        unsafe { &mut (*self.executing_fiber).stack }
    }

    pub fn get_call_frame<'a>(&'a self) -> &'a mut CallFrame {
        unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() }
    }

    /// Monotonic counter that changes whenever the frame stack or executing
    /// fiber changes.
    pub fn frame_epoch(&self) -> u64 {
        self.frame_epoch
    }

    pub fn bump_frame_epoch(&mut self) {
        self.frame_epoch = self.frame_epoch.wrapping_add(1);
    }

    pub fn current_pc(&self) -> usize {
        unsafe {
            (*self.executing_fiber)
                .call_frames
                .last()
                .map(|f| f.pc)
                .unwrap_or(0)
        }
    }

    pub fn set_pc(&mut self, pc: usize) {
        if let Some(frame) = unsafe { (*self.executing_fiber).call_frames.last_mut() } {
            frame.pc = pc;
        }
    }

    pub fn current_chunk_id(&self) -> Option<usize> {
        unsafe {
            (*self.executing_fiber).call_frames.last().map(|frame| {
                let chunk = &*(*frame.closure).chunk;
                chunk.native_id
            })
        }
    }

    pub fn frame_depth(&self) -> usize {
        unsafe { (*self.executing_fiber).call_frames.len() }
    }

    pub fn frame_bottom(&self) -> usize {
        unsafe {
            (*self.executing_fiber)
                .call_frames
                .last()
                .map(|frame| frame.bottom)
                .unwrap_or(0)
        }
    }

    pub fn current_chunk_constant(&self, index: usize) -> Option<Value> {
        unsafe {
            let frame = (*self.executing_fiber).call_frames.last()?;
            let chunk = &*(*frame.closure).chunk;
            chunk.constants.get(index).cloned()
        }
    }

    /// Return a retained copy of the global whose name is the constant at
    /// `index`.
    pub fn get_global_value(&mut self, index: usize) -> Value {
        let name = self
            .current_chunk_constant(index)
            .expect("global name constant is missing");
        if let ValueKind::String(name) = name.kind() {
            self.global
                .last()
                .and_then(|globals| globals.get(&name))
                .cloned()
                .unwrap_or(Value::Nil)
        } else {
            Value::Nil
        }
    }

    /// Store `value` in the global whose name is the constant at `index`.
    pub fn set_global_value(&mut self, index: usize, value: Value) {
        let name = self
            .current_chunk_constant(index)
            .expect("global name constant is missing");
        if let ValueKind::String(name) = name.kind() {
            self.global.last_mut().unwrap().insert(name, value);
        }
    }

    /// Build a closure from child chunk `index` (`LoadChunk`).
    pub fn load_chunk(&mut self, index: usize, next_pc: Option<usize>) -> EvalResult {
        let (bottom, current_closure, cur_this_ref) = {
            let frame = self.get_call_frame();
            (
                frame.bottom,
                frame.closure,
                frame.bound_this.or(unsafe { (*frame.closure).this_ref }),
            )
        };
        let chunk = self.get_chunk(index) as *const Chunk;
        let mut upvalues = Vec::new();
        for upval_decl in unsafe { &*chunk }.upvalues.iter() {
            match upval_decl {
                UpValueDecl::Ref(idx, _) => {
                    if let Some(pos) = self.upvalues.iter().position(|p| match unsafe { &**p } {
                        UpValueObject {
                            marked: _,
                            value: UpValue::Ref(idx2),
                        } => *idx2 == (*idx) + bottom,
                        _ => false,
                    }) {
                        upvalues.push(self.upvalues[pos]);
                    } else {
                        let upv = self.new_upvalue_object((*idx) + bottom);
                        upvalues.push(upv);
                    }
                }
                UpValueDecl::RefUpValue(idx, _) => {
                    let upv = unsafe { (&(*current_closure).upvalues)[*idx] };
                    upvalues.push(upv);
                }
            }
        }
        let closure = Closure {
            marked: false,
            chunk,
            upvalues,
            this_ref: cur_this_ref,
        };
        let mut boxed_closure = Box::new(closure);
        let pointer = boxed_closure.as_mut() as *mut Closure;
        self.objects.push(boxed_closure);
        self.get_stack().push(Value::Closure(pointer));
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Return the receiver bound to the current frame (`GetThis`).
    pub fn get_this_value(&mut self) -> Value {
        let frame = self.get_call_frame();
        let this = frame
            .bound_this
            .or(unsafe { (*frame.closure).this_ref })
            .expect("`this` is not bound in this frame");
        Value::Instance(this)
    }

    /// Push the current frame's variadic arguments (`UnpackVA`).
    pub fn unpack_va(&mut self, next_pc: Option<usize>) {
        let args = self.get_call_frame().va_args.clone();
        let stack = self.get_stack();
        for arg in args {
            stack.push(arg);
        }
        self.advance_pc(next_pc);
    }

    /// Create an array from the top `count` stack values (`InitArray`).
    pub fn init_array(&mut self, count: usize, next_pc: Option<usize>) -> EvalResult {
        self.run_gc()?;
        let array = self.new_array(count);
        self.get_stack().push(Value::Array(array));
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Create a dictionary from the top `count` key/value pairs (`InitDict`).
    pub fn init_dict(&mut self, count: usize, next_pc: Option<usize>) -> EvalResult {
        self.run_gc()?;
        let dict = self.new_dict(count);
        self.get_stack().push(Value::Dictionary(dict));
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Build a range from the top two stack values.
    pub fn make_range(&mut self, closed: bool, next_pc: Option<usize>) -> EvalResult {
        let (start, end) = self.stack_get_number()?;
        let end = if closed { end + 1.0 } else { end };
        self.get_stack().push(Value::Range(start, end));
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Evaluate `instance is Class`.
    pub fn class_is(&mut self, next_pc: Option<usize>) -> EvalResult {
        let klass = self.get_stack().pop().unwrap();
        let instance = self.get_stack().pop().unwrap();
        if let ValueKind::Klass(klass) = klass.kind() {
            if let ValueKind::Instance(instance) = instance.kind() {
                unsafe {
                    let mut current = (*instance).klass;
                    let mut is_class = false;
                    while current != null_mut() {
                        if current == klass {
                            is_class = true;
                            break;
                        }
                        current = (*current).super_klass;
                    }
                    self.get_stack().push(Value::Bool(is_class));
                }
            } else {
                self.get_stack().push(Value::Bool(false));
            }
            self.advance_pc(next_pc);
            Ok(())
        } else {
            Err(EvalError::TypeError(self.eval_err_str(
                "`is` can ONLY check classes, r-hand must be Class",
            )))
        }
    }

    /// Create an empty class object.
    pub fn init_class(&mut self, next_pc: Option<usize>) {
        let mut class = Box::new(Klass {
            marked: false,
            super_klass: null_mut(),
            methods: HashMap::new(),
        });
        let pointer = class.as_mut() as *mut Klass;
        self.objects.push(class);
        self.get_stack().push(Value::Klass(pointer));
        self.advance_pc(next_pc);
    }

    /// Apply a superclass to a class object (`ClassExtend`).
    pub fn class_extend(&mut self, next_pc: Option<usize>) -> EvalResult {
        let super_klass = self.get_stack().pop().unwrap();
        let klass = self.get_stack().pop().unwrap();
        if let ValueKind::Klass(klass) = klass.kind() {
            if let ValueKind::Klass(super_klass) = super_klass.kind() {
                unsafe {
                    (*klass).super_klass = super_klass;
                    for (name, closure) in &(*super_klass).methods {
                        (*klass).methods.insert(name.clone(), closure.clone());
                    }
                }
                self.get_stack().push(Value::Klass(klass));
                self.advance_pc(next_pc);
                Ok(())
            } else {
                Err(EvalError::TypeError(
                    self.eval_err_str("Class can only extend Class"),
                ))
            }
        } else {
            Err(EvalError::Error(
                self.eval_err_str("Class can only extend Class"),
            ))
        }
    }

    /// Attach a method closure to a class object (`AddMethod`).
    pub fn add_method(&mut self, next_pc: Option<usize>) -> EvalResult {
        let method = self.get_stack().pop().unwrap();
        let name = self.get_stack().pop().unwrap();
        let klass = self.get_stack().pop().unwrap();
        if let ValueKind::Klass(klass) = klass.kind() {
            if let ValueKind::String(name) = name.kind() {
                if let ValueKind::Closure(method) = method.kind() {
                    unsafe {
                        (*klass).methods.insert(name, Value::Closure(method));
                    }
                }
            }
            self.get_stack().push(Value::Klass(klass));
            self.advance_pc(next_pc);
            Ok(())
        } else {
            Err(EvalError::Error(
                self.eval_err_str("method target is not a Class"),
            ))
        }
    }

    /// Look up a method on the receiver's superclass (`GetSuperMethod`).
    pub fn get_super_method(&mut self, next_pc: Option<usize>) -> EvalResult {
        let index = self.get_stack().pop().unwrap();
        let collection = self.get_stack().pop().unwrap();
        if let ValueKind::Instance(instance) = collection.kind() {
            if let ValueKind::String(name) = index.kind() {
                unsafe {
                    let mut super_class = (*instance).klass;
                    while super_class != null_mut() {
                        super_class = (*super_class).super_klass;
                        if super_class == null_mut() {
                            break;
                        }
                        if let Some(method) = (*super_class).methods.get(&name) {
                            if let ValueKind::Closure(method) = method.kind() {
                                let mut bound = (*method).clone();
                                bound.this_ref = Some(instance);
                                let mut boxed = Box::new(bound);
                                let pointer = boxed.as_mut() as *mut Closure;
                                self.objects.push(boxed);
                                self.get_stack().push(Value::Closure(pointer));
                                self.advance_pc(next_pc);
                                return Ok(());
                            }
                        }
                    }
                }
                Err(EvalError::VariableNotFound(
                    self.eval_err_str("method not found"),
                ))
            } else {
                Err(EvalError::TypeError(
                    self.eval_err_str("Instance can only be indexed by String"),
                ))
            }
        } else {
            Err(EvalError::Error(self.eval_err_str("invalid super lookup")))
        }
    }

    /// Convert the top stack value into an iterator (`Iterator`).
    pub fn make_iterator(&mut self, next_pc: Option<usize>) -> EvalResult {
        let iterator = self.get_stack().pop().unwrap();
        match iterator.kind() {
            ValueKind::Range(left, right) => {
                self.get_stack().push(Value::Range(left, right));
            }
            ValueKind::String(string) => {
                self.get_stack().push(Value::StringIter(string, 0));
            }
            ValueKind::Array(array) => {
                self.get_stack().push(Value::ArrayIter(array, 0));
            }
            ValueKind::Dictionary(dict) => unsafe {
                let entries = (*dict)
                    .dict
                    .iter()
                    .map(|(key, value)| {
                        let entry = vec![Value::String(key.clone()), value.clone()];
                        let mut boxed = Box::new(Array {
                            marked: false,
                            array: entry,
                        });
                        let pointer = boxed.as_mut() as *mut Array;
                        self.objects.push(boxed);
                        Value::Array(pointer)
                    })
                    .collect();
                let mut boxed = Box::new(Array {
                    marked: false,
                    array: entries,
                });
                let pointer = boxed.as_mut() as *mut Array;
                self.objects.push(boxed);
                self.get_stack().push(Value::ArrayIter(pointer, 0));
            },
            ValueKind::Instance(instance) => {
                let name = self.string_pool.creat_istring("__iter__");
                if let Some(value) = unsafe { (*instance).fields.get(&name) } {
                    let value = value.clone();
                    self.get_stack().push(value);
                } else if let Some(method) = unsafe { (*(*instance).klass).methods.get(&name) } {
                    if let ValueKind::Closure(method) = method.kind() {
                        self.get_stack().push(Value::Closure(method));
                        self.call_top_at(0, Some(instance), false, next_pc)?;
                        return Ok(());
                    }
                } else {
                    return Err(EvalError::VariableNotFound(
                        self.eval_err_str("method not found"),
                    ));
                }
            }
            _ => return Err(EvalError::TypeError(self.eval_err_str("not subscriptable"))),
        }
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Advance the top iterator and push its next value (`Next`).
    pub fn iterator_next(&mut self, next_pc: Option<usize>) -> EvalResult {
        let mut iterator = self.get_stack().pop().unwrap();
        match iterator.kind() {
            ValueKind::Range(left, right) => {
                let value = if (left - right).abs() < f64::EPSILON {
                    Value::Nil
                } else if left < right {
                    let next = left + 1.0;
                    iterator = Value::Range(next, right);
                    Value::Number(next - 1.0)
                } else {
                    let next = left - 1.0;
                    iterator = Value::Range(next, right);
                    Value::Number(next + 1.0)
                };
                self.get_stack().push(iterator);
                self.get_stack().push(value);
            }
            ValueKind::ArrayIter(array, index) => unsafe {
                if index >= (*array).array.len() {
                    self.get_stack().push(iterator);
                    self.get_stack().push(Value::Nil);
                } else {
                    let value = unsafe { (&(*array).array)[index].clone() };
                    self.get_stack().push(Value::ArrayIter(array, index + 1));
                    self.get_stack().push(value);
                }
            },
            ValueKind::StringIter(string, index) => {
                let next = string.get_inner().chars().skip(index).next();
                if let Some(ch) = next {
                    let ch = format!("{ch}");
                    let ch = self.string_pool.creat_istring(&ch);
                    self.get_stack().push(Value::StringIter(string, index + 1));
                    self.get_stack().push(Value::String(ch));
                } else {
                    self.get_stack().push(Value::StringIter(string, index));
                    self.get_stack().push(Value::Nil);
                }
            }
            ValueKind::Instance(instance) => {
                self.get_stack().push(iterator);
                let name = self.string_pool.creat_istring("__next__");
                if let Some(value) = unsafe { (*instance).fields.get(&name) } {
                    let value = value.clone();
                    self.get_stack().push(value);
                } else if let Some(method) = unsafe { (*(*instance).klass).methods.get(&name) } {
                    if let ValueKind::Closure(method) = method.kind() {
                        self.get_stack().push(Value::Closure(method));
                        self.call_top_at(0, Some(instance), false, next_pc)?;
                        return Ok(());
                    }
                } else {
                    return Err(EvalError::VariableNotFound(
                        self.eval_err_str("method not found"),
                    ));
                }
            }
            _ => {
                return Err(EvalError::VariableNotFound(
                    self.eval_err_str("not a Iterator Object"),
                ))
            }
        }
        self.advance_pc(next_pc);
        Ok(())
    }

    /// Call the value at `stack.len() - argc - 1`.
    pub fn call_value(&mut self, argc: usize, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let callee = stack[stack.len() - argc - 1].kind();
        match callee {
            ValueKind::Closure(_) | ValueKind::NativeFunction(_) => {
                self.call_top_at(argc, None, false, next_pc)?;
            }
            ValueKind::Klass(klass) => {
                let class_index = stack.len() - argc - 1;
                let mut instance = Box::new(Instance {
                    marked: false,
                    klass,
                    fields: HashMap::new(),
                });
                let pointer = instance.as_mut() as *mut Instance;
                self.objects.push(instance);
                let name = self.string_pool.creat_istring("__init__");
                if let Some(method) = unsafe { (*klass).methods.get(&name) } {
                    if let ValueKind::Closure(method) = method.kind() {
                        stack[class_index] = Value::Closure(method);
                        self.call_top_at(argc, Some(pointer), false, next_pc)?;
                    }
                } else {
                    for _ in 0..argc {
                        stack.pop();
                    }
                    stack.pop();
                    stack.push(Value::Instance(pointer));
                    self.advance_pc(next_pc);
                }
            }
            _ => {
                return Err(EvalError::CallError(
                    self.eval_err_str("calling object which is not Callable"),
                ))
            }
        }
        Ok(())
    }

    /// Pop the current call frame (`Return`).  The caller's frame is already
    /// positioned at the instruction following the call.
    pub fn do_return(&mut self) -> Result<Step, EvalError> {
        let discard_return_value = self.get_call_frame().discard_return_value;
        let callframe = unsafe { (*self.executing_fiber).call_frames.pop().unwrap() };
        self.bump_frame_epoch();
        let chunk = unsafe { &*(*callframe.closure).chunk };
        let mut kept_upvalues = Vec::new();
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        for upv in self.upvalues.iter_mut() {
            let mut escape = false;
            let mut index = 0;
            if let UpValue::Ref(value_index) = unsafe { &(**upv).value } {
                if *value_index >= callframe.bottom {
                    escape = true;
                    index = *value_index;
                }
            }
            if escape {
                unsafe {
                    (**upv).value = UpValue::Closed(stack[index].clone());
                }
            } else {
                kept_upvalues.push(*upv);
            }
        }
        self.upvalues = kept_upvalues;

        if callframe.bottom + chunk.num_locals == stack.len() {
            for _ in callframe.bottom..stack.len() {
                stack.pop();
            }
            stack.pop();
            if !discard_return_value {
                stack.push(Value::Nil);
            }
        } else {
            let value = stack.pop().unwrap();
            for _ in callframe.bottom..stack.len() {
                stack.pop();
            }
            stack.pop();
            if !discard_return_value {
                stack.push(value);
            }
        }

        unsafe {
            if (*self.executing_fiber).call_frames.is_empty() {
                let returning = self.executing_fiber;
                let previous = (*self.executing_fiber).prev;
                if previous != null_mut() {
                    self.executing_fiber = previous;
                    self.bump_frame_epoch();
                    if (*returning).state == FiberState::Loader {
                        let namespace = self.global.pop().unwrap();
                        let mut dict = Box::new(Dict {
                            marked: false,
                            dict: namespace,
                        });
                        let pointer = dict.as_mut() as *mut Dict;
                        self.objects.push(dict);
                        self.get_stack().push(Value::Dictionary(pointer));
                    } else {
                        (*returning).state = FiberState::Finished;
                        (*previous).state = FiberState::Running;
                        self.get_stack().push(Value::Nil);
                    }
                } else {
                    return Ok(Step::Finished);
                }
            }
        }
        Ok(Step::Continue)
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    pub fn has_error(&self) -> bool {
        self.last_error.is_some()
    }

    pub fn set_last_error(&mut self, err: String) {
        self.last_error = Some(err);
    }

    pub fn set_precompiled_modules(&mut self, modules: HashMap<String, Chunk>) {
        self.precompiled_modules = Some(modules);
    }

    pub fn has_precompiled_modules(&self) -> bool {
        self.precompiled_modules.is_some()
    }

    pub fn string_pool_mut(&mut self) -> &mut StringPool {
        &mut self.string_pool
    }
    pub fn run(&mut self) -> EvalResult {
        loop {
            match self.step()? {
                Step::Continue => {}
                Step::Finished => return Ok(()),
            }
        }
    }

    /// Decode and execute exactly one instruction.  `run` is implemented on top
    /// of this method; the AOT runtime uses dedicated entry points instead.
    pub fn step(&mut self) -> Result<Step, EvalError> {
        let call_frame = unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() };
        let closure = call_frame.closure;
        let mut stack = unsafe { &mut (*self.executing_fiber).stack };
        let instr = match unsafe { (&*(&*call_frame.closure).chunk).decode(call_frame.pc) } {
            Ok((instr, _len)) => instr,
            Err(opcode) => {
                return Err(EvalError::Error(self.eval_err_str(
                    format!("unknown or truncated instruction {opcode:#04x}").as_ref(),
                )))
            }
        };
        if self.debug {
            dbg!(&instr);
            dbg!(&stack);
            dbg!(&call_frame);
        }
        let num_locals = unsafe { (*(*call_frame.closure).chunk).num_locals };
        if stack.len() < num_locals {
            panic!(
                "stack corrupted, VM implementation issue {} < {}",
                stack.len(),
                num_locals
            );
        }
        let _ = stack;
        let _ = closure;
        self.exec(instr)
    }

    /// Execute a single decoded instruction.  This is the interpreter's core;
    /// the AOT backend uses dedicated entry points instead.
    pub fn exec(&mut self, instr: Instr) -> Result<Step, EvalError> {
        let call_frame = unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() };
        let closure = call_frame.closure;
        let mut stack = unsafe { &mut (*self.executing_fiber).stack };
        match instr {
            Instr::Add => {
                let opr2 = stack.pop().unwrap();
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        if let ValueKind::Number(b) = opr2.kind() {
                            let res = a + b;
                            stack.push(Value::Number(res));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `+`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::String(a) => {
                        if let ValueKind::String(b) = opr2.kind() {
                            let mut res = a.get_inner().to_owned();
                            res += b.get_inner();
                            let istring = self.string_pool.creat_istring(&res);
                            stack.push(Value::String(istring));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `+`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::Array(a) => {
                        if let ValueKind::Array(b) = opr2.kind() {
                            unsafe {
                                let mut new_arr = Vec::new();
                                (*a).array.iter().for_each(|x| new_arr.push(x.clone()));
                                (*b).array.iter().for_each(|x| new_arr.push(x.clone()));
                                let mut b_new_arr = Box::new(Array {
                                    marked: false,
                                    array: new_arr,
                                });
                                let p_new_arr = b_new_arr.as_mut() as *mut Array;
                                self.objects.push(b_new_arr);
                                stack.push(Value::Array(p_new_arr));
                            }
                            self.pc_add();
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `+`"),
                            ));
                        }
                    }
                    ValueKind::Instance(p_instance) => {
                        // a + b => a.__add__(b)
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__add__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__add__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    stack.push(opr2);
                                    self.call_top(1, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__add__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `+`"),
                        ))
                    }
                }
            }
            Instr::Sub => {
                let opr2 = stack.pop().unwrap();
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        if let ValueKind::Number(b) = opr2.kind() {
                            let res = a - b;
                            stack.push(Value::Number(res));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `-`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // a + b => a.__add__(b)
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__sub__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__sub__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    stack.push(opr2);
                                    self.call_top(1, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__sub__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `-`"),
                        ))
                    }
                }
            }
            Instr::Mul => {
                let opr2 = stack.pop().unwrap();
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        if let ValueKind::Number(b) = opr2.kind() {
                            let res = a * b;
                            stack.push(Value::Number(res));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `*`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // a + b => a.__add__(b)
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__mul__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__mul__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    stack.push(opr2);
                                    self.call_top(1, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__mul__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `*`"),
                        ))
                    }
                }
            }
            Instr::Div => {
                let opr2 = stack.pop().unwrap();
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        if let ValueKind::Number(b) = opr2.kind() {
                            if b < 1e-5 {
                                return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                            }
                            let res = a / b;
                            stack.push(Value::Number(res));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `/`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // a + b => a.__add__(b)
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__div__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__div__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    stack.push(opr2);
                                    self.call_top(1, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__div__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `/`"),
                        ))
                    }
                }
            }
            Instr::Mod => {
                let opr2 = stack.pop().unwrap();
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        if let ValueKind::Number(b) = opr2.kind() {
                            if b < 1e-5 {
                                return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                            }
                            let res = a % b;
                            stack.push(Value::Number(res));
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("unsupported operation on `%`"),
                            ));
                        }
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // a + b => a.__add__(b)
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__mod__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__mod__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    stack.push(opr2);
                                    self.call_top(1, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__mod__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `%`"),
                        ))
                    }
                }
            }
            Instr::Negate => {
                let opr1 = stack.pop().unwrap();
                match opr1.kind() {
                    ValueKind::Number(a) => {
                        stack.push(Value::Number(-a));
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // -a => a.__neg__()
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__neg__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`__neg__` defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    self.call_top(0, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__neg__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported operation on `-(pfx)`"),
                        ))
                    }
                }
            }
            Instr::Gt => {
                self.binary_predicate_impl(|x, y| x > y, |s1, s2| s1 > s2, "__gt__")?;
            }
            Instr::Lt => {
                self.binary_predicate_impl(|x, y| x < y, |s1, s2| s1 < s2, "__lt__")?;
            }
            Instr::Ge => {
                self.binary_predicate_impl(|x, y| x >= y, |s1, s2| s1 >= s2, "__ge__")?;
            }
            Instr::Le => {
                self.binary_predicate_impl(|x, y| x <= y, |s1, s2| s1 <= s2, "__le__")?;
            }
            Instr::Eq => {
                self.binary_predicate_impl(
                    |x, y| (x - y).abs() < 1e-5,
                    |s1, s2| s1 == s2,
                    "__eq__",
                )?;
            }
            Instr::Ne => {
                self.binary_predicate_impl(
                    |x, y| (x - y).abs() >= 1e-5,
                    |s1, s2| s1 != s2,
                    "__ne__",
                )?;
            }
            Instr::Not => {
                let opr1 = self.get_stack().pop().unwrap();
                match opr1.kind() {
                    ValueKind::Bool(a) => {
                        let res = !a;
                        self.get_stack().push(Value::Bool(res));
                        self.pc_add();
                    }
                    ValueKind::Instance(p_instance) => {
                        // not a -> a.__not__()
                        let instance = unsafe { &mut *p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__not__");
                        if let Some(_v) = instance.fields.get(&protocol_func_name) {
                            return Err(EvalError::CallError(
                                self.eval_err_str("`not` operation defined as field of Instance"),
                            ));
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let ValueKind::Closure(method) = method.kind() {
                                    stack.push(Value::Closure(method));
                                    self.call_top(0, Some(p_instance), false)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("`__not__` method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("unsupported `not` operation"),
                        ))
                    }
                }
            }
            Instr::Or => {
                let (opr1, opr2) = self.stack_get_bool()?;
                stack.push(Value::Bool(opr1 || opr2));
                self.pc_add();
            }
            Instr::And => {
                let (opr1, opr2) = self.stack_get_bool()?;
                stack.push(Value::Bool(opr1 && opr2));
                self.pc_add();
            }
            Instr::ClassIs => {
                self.class_is(None)?;
            }
            Instr::MakeRange => {
                self.make_range(false, None)?;
            }
            Instr::MakeRangeClosed => {
                self.make_range(true, None)?;
            }
            Instr::PushNil => {
                stack.push(Value::Nil);
                self.pc_add();
            }
            Instr::LoadTrue => {
                stack.push(Value::Bool(true));
                self.pc_add();
            }
            Instr::LoadFalse => {
                stack.push(Value::Bool(false));
                self.pc_add();
            }
            Instr::Pop => {
                stack.pop();
                self.pc_add();
            }
            Instr::Swap2 => {
                let t1 = stack.pop().unwrap();
                let t2 = stack.pop().unwrap();
                stack.push(t1);
                stack.push(t2);
                self.pc_add();
            }
            Instr::LoadChunk(x) => {
                self.load_chunk(x, None)?;
            }
            Instr::Load(x) => {
                let v = self.get_constant(x);
                let val = v.clone();
                stack.push(val);
                self.pc_add();
            }
            Instr::GetGlobal(x) => {
                let value = self.get_global_value(x);
                stack.push(value);
                self.pc_add();
            }
            Instr::SetGlobal(x) => {
                let value = stack.pop().unwrap();
                self.set_global_value(x, value);
                self.pc_add();
            }
            Instr::GetLocal(x) => {
                let callframe = call_frame;
                let bottom = callframe.bottom;
                let v = stack[x + bottom].clone();
                stack.push(v);
                self.pc_add();
            }
            Instr::SetLocal(x) => {
                let callframe = call_frame;
                let bottom = callframe.bottom;
                let v = stack.pop().unwrap();
                stack[x + bottom] = v;
                self.pc_add();
            }
            Instr::GetUpValue(x) => {
                let upv = self.get_upvalue(x);
                stack.push(upv);
                self.pc_add();
            }
            Instr::SetUpValue(x) => {
                let opr = stack.pop().unwrap();
                self.set_upvalue(x, opr);
                self.pc_add();
            }
            Instr::InitArray(n) => {
                self.init_array(n, None)?;
            }
            Instr::InitDict(n) => {
                self.init_dict(n, None)?;
            }
            Instr::GetCollection => {
                self.exec_get_collection(None)?;
            }
            Instr::GetProperty => {
                self.exec_get_property(None)?;
            }
            Instr::InvokeProperty(argc) => {
                self.exec_invoke_property(argc, None)?;
            }
            Instr::InvokeSuper(argc) => {
                self.exec_invoke_super(argc, None)?;
            }
            Instr::SetCollection => {
                self.exec_set_collection(None)?;
            }
            Instr::SetProperty => {
                self.exec_set_property(None)?;
            }
            Instr::Jump(x) => {
                let last_pc = call_frame.pc as i32;
                let pc = (last_pc + x) as usize;
                call_frame.pc = pc;
            }
            Instr::JumpIfNot(x) => {
                let b = stack.last().unwrap().to_bool();
                if !b {
                    let last_pc = call_frame.pc as i32;
                    let pc = (last_pc + x) as usize;
                    call_frame.pc = pc;
                } else {
                    self.pc_add();
                }
            }
            Instr::JumpIfTrue(x) => {
                let b = stack.last().unwrap().to_bool();
                if b {
                    let last_pc = call_frame.pc as i32;
                    let pc = (last_pc + x) as usize;
                    call_frame.pc = pc;
                } else {
                    self.pc_add();
                }
            }

            Instr::Iterator => {
                self.make_iterator(None)?;
            }

            Instr::Next => {
                self.iterator_next(None)?;
            }
            Instr::Call(x) => {
                self.call_value(x, None)?;
            }

            Instr::Return => {
                return self.do_return();
            }
            Instr::GetSuperMethod => {
                self.get_super_method(None)?;
            }
            Instr::InitClass => {
                self.init_class(None);
            }
            Instr::ClassExtend => {
                self.class_extend(None)?;
            }
            Instr::AddMethod => {
                self.add_method(None)?;
            }
            Instr::GetThis => {
                let value = self.get_this_value();
                stack.push(value);
                self.pc_add();
            }
            Instr::Nop => {
                self.pc_add();
            }
            Instr::UnpackVA => {
                self.unpack_va(None);
            }
            i => {
                return Err(EvalError::Error(
                    self.eval_err_str(format!("unknown instruction {i:?}").as_ref()),
                ))
            }
        }
        Ok(Step::Continue)
    }

    fn binary_predicate_impl(
        &mut self,
        op: fn(f64, f64) -> bool,
        op_str: fn(&str, &str) -> bool,
        op_name: &str,
    ) -> Result<(), EvalError> {
        let opr2 = self.get_stack().pop().unwrap();
        let opr1 = self.get_stack().pop().unwrap();
        match opr1.kind() {
            ValueKind::Number(a) => {
                if let ValueKind::Number(b) = opr2.kind() {
                    let res = op(a, b);
                    self.get_stack().push(Value::Bool(res));
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("unsupported comparing operation"),
                    ));
                }
                self.pc_add();
            }
            ValueKind::String(s1) => {
                if let ValueKind::String(s2) = opr2.kind() {
                    let res = op_str(s1.get_inner(), s2.get_inner());
                    self.get_stack().push(Value::Bool(res));
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("unsupported comparing operation"),
                    ));
                }
                self.pc_add();
            }
            ValueKind::Instance(p_instance) => {
                // a op b => a.__op__(b)
                let instance = unsafe { &mut *p_instance };
                let protocol_func_name = self.string_pool.creat_istring(op_name);
                if let Some(v) = instance.fields.get(&protocol_func_name) {
                    return Err(EvalError::CallError(
                        self.eval_err_str("coparing operation defined as field of Instance"),
                    ));
                } else {
                    if let Some(method) =
                        unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                    {
                        if let ValueKind::Closure(method) = method.kind() {
                            self.get_stack().push(Value::Closure(method));
                            self.get_stack().push(opr2);
                            self.call_top(1, Some(p_instance), false)?;
                        } else {
                            unreachable!()
                        }
                    } else {
                        return Err(EvalError::VariableNotFound(
                            self.eval_err_str("comparing method not found"),
                        ));
                    }
                };
            }
            _ => {
                return Err(EvalError::TypeError(
                    self.eval_err_str("unsupported comparing operation"),
                ))
            }
        }
        Ok(())
    }
    /// `collection[index]`
    pub fn exec_get_collection(&mut self, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let idx = stack.pop().unwrap();
        let clct = stack.pop().unwrap();
        match clct.kind() {
            ValueKind::Array(p_array) => {
                if let ValueKind::Number(i) = idx.kind() {
                    if i < 0. {
                        return Err(EvalError::IndexOutOfBound(
                            self.eval_err_str("Array cannot be indexed by negative value"),
                        ));
                    }
                    let i = i as usize;
                    let arr = unsafe { &mut *p_array };
                    if i >= arr.array.len() {
                        return Err(EvalError::IndexOutOfBound(
                            self.eval_err_str("index >= length of array"),
                        ));
                    } else {
                        let elem = arr.array.get(i).unwrap().clone();
                        stack.push(elem);
                        self.advance_pc(next_pc);
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Array can only be indexed by Number"),
                    ));
                }
            }
            ValueKind::Dictionary(p_dict) => {
                if let ValueKind::String(s) = idx.kind() {
                    let m = unsafe { &mut *p_dict };
                    if let Some(v) = m.dict.get(&s) {
                        stack.push(v.clone());
                        self.advance_pc(next_pc);
                    } else {
                        return Err(EvalError::KeyError(
                            self.eval_err_str("unknown key to module"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Dict can only be indexed by String"),
                    ));
                }
            }
            ValueKind::Instance(p_instance) => {
                let protocol_name = self.string_pool.creat_istring("__index__");
                if let Some(method) = unsafe { (*(*p_instance).klass).methods.get(&protocol_name) }
                {
                    if let ValueKind::Closure(method) = method.kind() {
                        stack.push(Value::Closure(method));
                        stack.push(idx);
                        self.call_top_at(1, Some(p_instance), false, next_pc)?;
                    } else {
                        unreachable!()
                    }
                } else {
                    return Err(EvalError::VariableNotFound(
                        self.eval_err_str("`__index__` method not found"),
                    ));
                }
            }
            ValueKind::Module(p_module) => {
                if let ValueKind::String(s) = idx.kind() {
                    let m = unsafe { &mut *p_module };
                    if let Some(v) = m.dict.get(&s) {
                        stack.push(v.clone());
                        self.advance_pc(next_pc);
                    } else {
                        return Err(EvalError::KeyError(
                            self.eval_err_str("unknown key to module"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Module can only be indexed by String"),
                    ));
                }
            }
            v => {
                return Err(EvalError::TypeError(
                    self.eval_err_str(format!("{:?} can not be indexed", v).as_ref()),
                ));
            }
        };
        Ok(())
    }

    /// `collection.property`
    pub fn exec_get_property(&mut self, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let idx = stack.pop().unwrap();
        let clct = stack.pop().unwrap();
        match clct.kind() {
            ValueKind::Array(_) => {
                // `methods` on Array
                stack.push(clct);
                if let ValueKind::String(s) = idx.kind() {
                    let ext_name = self.get_builtin_type_extension_name("Array", s.get_inner());
                    let ext_method = self.global.last().unwrap()[&ext_name].clone();
                    stack.push(ext_method);
                    self.advance_pc(next_pc);
                } else {
                    return Err(EvalError::TypeError(self.eval_err_str(
                        "Array. syntax is used to call extension methods on it.",
                    )));
                }
            }
            ValueKind::Dictionary(p_dict) => {
                if let ValueKind::String(s) = idx.kind() {
                    let m = unsafe { &mut *p_dict };
                    if let Some(v) = m.dict.get(&s) {
                        stack.push(v.clone());
                        self.advance_pc(next_pc);
                    } else {
                        return Err(EvalError::KeyError(
                            self.eval_err_str("unknown key to module"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Dict can only be indexed by String"),
                    ));
                }
            }
            ValueKind::Instance(p_instance) => {
                if let ValueKind::String(i) = idx.kind() {
                    let instance = unsafe { &mut *p_instance };
                    if let Some(v) = instance.fields.get(&i) {
                        stack.push(v.clone());
                        self.advance_pc(next_pc);
                    } else if let Some(method) = unsafe { (*instance.klass).methods.get(&i) } {
                        if let ValueKind::Closure(method) = method.kind() {
                            let mut binded_closure = unsafe { (*method).clone() };
                            binded_closure.this_ref = Some(p_instance);
                            let mut b_binded_closure = Box::new(binded_closure);
                            let p_binded_closure = b_binded_closure.as_mut() as *mut Closure;
                            self.objects.push(b_binded_closure);
                            let v = Value::Closure(p_binded_closure);
                            stack.push(v);
                            self.advance_pc(next_pc);
                        } else {
                            unreachable!()
                        }
                    } else {
                        return Err(EvalError::VariableNotFound(
                            self.eval_err_str("method not found"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Instance can only be indexed by String"),
                    ));
                }
            }
            ValueKind::Module(p_module) => {
                if let ValueKind::String(s) = idx.kind() {
                    let m = unsafe { &mut *p_module };
                    if let Some(v) = m.dict.get(&s) {
                        stack.push(v.clone());
                        self.advance_pc(next_pc);
                    } else {
                        return Err(EvalError::KeyError(
                            self.eval_err_str("unknown key to module"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Module can only be indexed by String"),
                    ));
                }
            }
            v => {
                return Err(EvalError::TypeError(
                    self.eval_err_str(format!("{:?} can not be indexed", v).as_ref()),
                ));
            }
        };
        Ok(())
    }

    /// `receiver.property(args...)`
    ///
    /// Fast path for the common `obj.method(...)` form.  The stack layout is
    /// `[receiver, name, arg0, ..., argN-1]`; the name is the constant loaded by
    /// the compiler just before this instruction.
    ///
    /// Instance methods are called through the original closure stored in the
    /// class method table.  The receiver is carried by
    /// [`CallFrame::bound_this`] instead of allocating a bound closure.
    pub fn exec_invoke_property(&mut self, argc: usize, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let recv_idx = stack.len() - argc - 2;
        let name = match stack[recv_idx + 1].kind() {
            ValueKind::String(s) => s,
            _ => {
                return Err(EvalError::TypeError(
                    self.eval_err_str("property name must be a String"),
                ))
            }
        };
        let receiver = stack[recv_idx].clone();
        match receiver.kind() {
            ValueKind::Instance(p_instance) => {
                let instance = unsafe { &mut *p_instance };
                if let Some(v) = instance.fields.get(&name) {
                    // Field values are called without binding `this`, matching
                    // the previous `GetProperty` + `Call` behaviour.
                    let v = v.clone();
                    stack[recv_idx] = v;
                    stack.remove(recv_idx + 1);
                    self.call_top_at(argc, None, false, next_pc)?;
                } else if let Some(method) = unsafe { (*instance.klass).methods.get(&name) } {
                    if let ValueKind::Closure(method) = method.kind() {
                        stack[recv_idx] = Value::Closure(method);
                        stack.remove(recv_idx + 1);
                        self.call_top_at(argc, Some(p_instance), false, next_pc)?;
                    } else {
                        unreachable!()
                    }
                } else {
                    return Err(EvalError::VariableNotFound(
                        self.eval_err_str("method not found"),
                    ));
                }
            }
            ValueKind::Array(_) => {
                // Extension methods are native functions that expect the
                // receiver directly below the callee, so keep both slots.
                stack[recv_idx + 1] = {
                    let ext_name = self.get_builtin_type_extension_name("Array", name.get_inner());
                    self.global.last().unwrap()[&ext_name].clone()
                };
                self.call_top_at(argc, None, false, next_pc)?;
            }
            ValueKind::Dictionary(p_dict) | ValueKind::Module(p_dict) => {
                let m = unsafe { &mut *p_dict };
                let v = match m.dict.get(&name) {
                    Some(v) => v.clone(),
                    None => {
                        return Err(EvalError::KeyError(
                            self.eval_err_str("unknown key to module"),
                        ))
                    }
                };
                stack[recv_idx] = v;
                stack.remove(recv_idx + 1);
                self.call_top_at(argc, None, false, next_pc)?;
            }
            v => {
                return Err(EvalError::TypeError(
                    self.eval_err_str(format!("{:?} can not be indexed", v).as_ref()),
                ));
            }
        };
        Ok(())
    }

    /// `collection[index] = value`
    pub fn exec_set_collection(&mut self, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let val = stack.pop().unwrap();
        let idx = stack.pop().unwrap();
        let clct = stack.pop().unwrap();
        match clct.kind() {
            ValueKind::Array(p_array) => {
                if let ValueKind::Number(i) = idx.kind() {
                    if i < 0. {
                        return Err(EvalError::IndexOutOfBound(
                            self.eval_err_str("Array cannot be indexed by negative value"),
                        ));
                    }
                    let i = i as usize;
                    let arr = unsafe { &mut *p_array };
                    if let Some(elem) = arr.array.get_mut(i) {
                        *elem = val;
                    } else {
                        return Err(EvalError::IndexOutOfBound(
                            self.eval_err_str("Array index out of bound"),
                        ));
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Array can only be indexed by Number"),
                    ));
                }
                self.advance_pc(next_pc);
            }
            ValueKind::Dictionary(p_dict) => {
                if let ValueKind::String(i) = idx.kind() {
                    let dict = unsafe { &mut *p_dict };
                    if let Some(elem) = dict.dict.get_mut(&i) {
                        *elem = val;
                    } else {
                        dict.dict.insert(i, val);
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Dict can only be indexed by String"),
                    ));
                }
                self.advance_pc(next_pc);
            }
            ValueKind::Instance(p_instance) => {
                // this.__assign__(idx, val)
                let protocol_name = self.string_pool.creat_istring("__assign__");
                if let Some(method) = unsafe { (*(*p_instance).klass).methods.get(&protocol_name) }
                {
                    if let ValueKind::Closure(method) = method.kind() {
                        stack.push(Value::Closure(method));
                        stack.push(idx);
                        stack.push(val);
                        self.call_top_at(2, Some(p_instance), true, next_pc)?;
                    } else {
                        unreachable!()
                    }
                } else {
                    return Err(EvalError::VariableNotFound(
                        self.eval_err_str("`__assign__` method not found"),
                    ));
                }
            }
            v => {
                return Err(EvalError::TypeError(self.eval_err_str(
                    format!("{:?} cannot be indexed and assigned to", v).as_ref(),
                )));
            }
        }
        Ok(())
    }

    /// `collection.property = value`
    pub fn exec_set_property(&mut self, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let val = stack.pop().unwrap();
        let idx = stack.pop().unwrap();
        let clct = stack.pop().unwrap();
        match clct.kind() {
            ValueKind::Dictionary(p_dict) => {
                if let ValueKind::String(i) = idx.kind() {
                    let dict = unsafe { &mut *p_dict };
                    if let Some(elem) = dict.dict.get_mut(&i) {
                        *elem = val;
                    } else {
                        dict.dict.insert(i, val);
                    }
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Dict can only be indexed by String"),
                    ));
                }
                self.advance_pc(next_pc);
            }
            ValueKind::Instance(p_instance) => {
                if let ValueKind::String(i) = idx.kind() {
                    unsafe { (*p_instance).fields.insert(i.clone(), val) };
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("Instance can only be indexed by String"),
                    ));
                }
                self.advance_pc(next_pc);
            }
            v => {
                return Err(EvalError::TypeError(self.eval_err_str(
                    format!("{:?} cannot have properties assigned to", v).as_ref(),
                )));
            }
        }
        Ok(())
    }

    /// `super.property(args...)`
    ///
    /// Like [`Vm::exec_invoke_property`], but the method lookup starts at the
    /// receiver's superclass.
    pub fn exec_invoke_super(&mut self, argc: usize, next_pc: Option<usize>) -> EvalResult {
        let stack = unsafe { &mut (*self.executing_fiber).stack };
        let recv_idx = stack.len() - argc - 2;
        let name = match stack[recv_idx + 1].kind() {
            ValueKind::String(s) => s,
            _ => {
                return Err(EvalError::TypeError(
                    self.eval_err_str("property name must be a String"),
                ))
            }
        };
        let receiver = stack[recv_idx].clone();
        let p_instance = match receiver.kind() {
            ValueKind::Instance(p_instance) => p_instance,
            _ => unreachable!(),
        };

        let mut super_class = unsafe { (*(*p_instance).klass).super_klass };
        while super_class != ptr::null_mut() {
            if let Some(method) = unsafe { (*super_class).methods.get(&name) } {
                if let ValueKind::Closure(method) = method.kind() {
                    stack[recv_idx] = Value::Closure(method);
                    stack.remove(recv_idx + 1);
                    self.call_top_at(argc, Some(p_instance), false, next_pc)?;
                    return Ok(());
                }
            }
            super_class = unsafe { (*super_class).super_klass };
        }
        Err(EvalError::VariableNotFound(
            self.eval_err_str("method not found"),
        ))
    }

    /// Advance the current frame's program counter.
    ///
    /// `next_pc` is supplied by the AOT backend, which already knows the next
    /// instruction offset.  The interpreter passes `None` and decodes the
    /// current instruction to find its length.
    #[inline]
    fn advance_pc(&mut self, next_pc: Option<usize>) {
        match next_pc {
            Some(pc) => self.set_pc(pc),
            None => self.pc_add(),
        }
    }

    /// Call the value at `stack.len() - arg_cnt - 1`.
    ///
    /// `bound_this` is stored in the newly pushed frame for direct method
    /// invocations.  It takes precedence over `closure.this_ref` in
    /// [`Instr::GetThis`].
    fn call_top(
        &mut self,
        arg_cnt: usize,
        bound_this: Option<*mut Instance>,
        discard_return_value: bool,
    ) -> Result<(), EvalError> {
        self.call_top_at(arg_cnt, bound_this, discard_return_value, None)
    }

    /// Shared implementation of `call_top` for the interpreter and the AOT
    /// runtime.  The caller frame is advanced before a new frame is pushed, so
    /// the interpreter and AOT agree on where execution resumes.
    fn call_top_at(
        &mut self,
        arg_cnt: usize,
        bound_this: Option<*mut Instance>,
        discard_return_value: bool,
        next_pc: Option<usize>,
    ) -> Result<(), EvalError> {
        let x = arg_cnt;
        let stack = self.get_stack();
        let callee = stack[stack.len() - x - 1].kind();
        match callee {
            ValueKind::Closure(p_closure) => {
                let mut packed_va_list = Vec::new();
                let chunk = unsafe { &*((*p_closure).chunk) };
                if chunk.parameter_num != x {
                    if chunk.parameter_num < x && chunk.is_va {
                        for idx in (stack.len() - x + chunk.parameter_num)..stack.len() {
                            packed_va_list.push(stack[idx].clone());
                        }
                    } else {
                        return Err(EvalError::CallError(
                            self.eval_err_str(
                                format!("wrong number of argument {x}/{}", chunk.parameter_num)
                                    .as_ref(),
                            ),
                        ));
                    }
                }
                let mut call_frame = CallFrame::new(stack.len() - x, p_closure, packed_va_list);
                call_frame.bound_this = bound_this;
                call_frame.discard_return_value = discard_return_value;
                // va_list arg should be pop
                for _ in 0..x - chunk.parameter_num {
                    stack.pop();
                }
                self.advance_pc(next_pc);
                self.reserve_local(chunk.num_locals - chunk.parameter_num);
                unsafe {
                    (*self.executing_fiber).call_frames.push(call_frame);
                }
                self.bump_frame_epoch();
            }
            ValueKind::NativeFunction(f) => {
                // Advance before running the native function so that a fiber
                // switch leaves this frame at the instruction after the call.
                self.advance_pc(next_pc);
                let f = unsafe { std::mem::transmute::<*mut u8, NativeFunction>(f) };
                f(self, x);
            }
            _ => {
                return Err(EvalError::CallError(
                    self.eval_err_str("calling object which is not Callable"),
                ));
            }
        }
        Ok(())
    }
    fn new_upvalue_object(&mut self, idx: usize) -> *mut UpValueObject {
        let mut ret = Box::new(UpValueObject {
            marked: false,
            value: UpValue::Ref(idx),
        });
        let pointer = ret.as_mut() as *mut UpValueObject;
        self.objects.push(ret);
        self.upvalues.push(pointer);
        pointer
    }
    fn new_array(&mut self, n: usize) -> *mut Array {
        let mut vec = Vec::new();
        for _ in 0..n {
            let v = self.get_stack().pop().unwrap();
            vec.push(v);
        }
        vec.reverse();
        let mut ret = Box::new(Array {
            marked: false,
            array: vec,
        });
        let pointer = ret.as_mut() as *mut Array;
        self.objects.push(ret);
        pointer
    }
    fn new_dict(&mut self, n: usize) -> *mut Dict {
        let mut dict = HashMap::new();
        for _ in 0..n {
            let v = self.get_stack().pop().unwrap();
            let k_wrap = self.get_stack().pop().unwrap();
            let k;
            if let ValueKind::String(s) = k_wrap.kind() {
                k = s;
            } else {
                panic!("dict key is supposed to be String, maybe wrong bytecodes emitted");
            }
            dict.insert(k, v);
        }
        let mut ret = Box::new(Dict {
            marked: false,
            dict,
        });
        let pointer = ret.as_mut() as *mut Dict;
        self.objects.push(ret);
        pointer
    }
    #[inline]
    fn reserve_local(&mut self, n: usize) {
        for _ in 0..n {
            self.get_stack().push(Value::Nil);
        }
    }
    #[inline]
    fn stack_get_number(&mut self) -> Result<(f64, f64), EvalError> {
        let (opr1, opr2);
        if let ValueKind::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil).kind() {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let ValueKind::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil).kind() {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_bool(&mut self) -> Result<(bool, bool), EvalError> {
        let (opr1, opr2);
        if let ValueKind::Bool(x) = self.get_stack().pop().unwrap_or(Value::Nil).kind() {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let ValueKind::Bool(x) = self.get_stack().pop().unwrap_or(Value::Nil).kind() {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_number1(&mut self) -> Result<f64, EvalError> {
        if let ValueKind::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil).kind() {
            Ok(x)
        } else {
            Err(EvalError::TypeError(self.eval_err_str("")))
        }
    }
    #[inline]
    fn get_constant(&mut self, idx: usize) -> &Value {
        let callframe = self.get_call_frame();
        let closure = unsafe { &*callframe.closure };
        let chunk = unsafe { &*closure.chunk };
        &chunk.constants[idx]
    }

    fn get_chunk(&mut self, idx: usize) -> &Chunk {
        let callframe = self.get_call_frame();
        let closure = unsafe { &*callframe.closure };
        let chunk = unsafe { &*closure.chunk };
        &chunk.chunks[idx]
    }
    #[inline]
    pub fn get_upvalue(&mut self, idx: usize) -> Value {
        let callframe = self.get_call_frame();
        let closure = unsafe { &mut *callframe.closure };
        let upv_obj = closure.upvalues[idx];
        let upv = unsafe { &(*upv_obj).value };
        match upv {
            UpValue::Ref(idx) => self.get_stack()[*idx].clone(),
            UpValue::Closed(value) => value.clone(),
        }
    }
    #[inline]
    pub fn set_upvalue(&mut self, idx: usize, v: Value) {
        let callframe = self.get_call_frame();
        let closure = unsafe { &mut *callframe.closure };
        let upv_obj = closure.upvalues[idx];
        let upv = unsafe { &mut (*upv_obj).value };
        match upv {
            UpValue::Ref(idx) => {
                self.get_stack()[*idx] = v;
            }
            UpValue::Closed(value) => *value = v,
        }
    }
    #[inline]
    fn pc_add(&mut self) {
        let callframe = self.get_call_frame();
        callframe.nxt()
    }
    #[inline]
    fn eval_err_str(&self, s: &str) -> String {
        let callframe = self.get_call_frame();
        let line = unsafe { (&*(&*callframe.closure).chunk).lines[callframe.pc] };
        format!("{s} in {line}")
    }
    fn run_gc(&mut self) -> EvalResult {
        if self.objects.len() < 128 {
            return Ok(());
        }
        // `main_fiber` is not stored in `self.objects`, so it is not demarked
        // by the sweep below. Demark it explicitly, otherwise its children
        // would be skipped after the first collection.
        self.main_fiber.demark();

        // Roots: the currently executing fiber (which also keeps its call
        // frame chain alive), the main fiber and all global namespaces.
        unsafe {
            mark_gc_object(&mut *self.executing_fiber);
        }
        mark_gc_object(&mut *self.main_fiber);
        for g in self.global.iter() {
            for v in g.values() {
                mark_value(v);
            }
        }
        // Open upvalues are owned by a fiber's stack/frames, but keep them
        // alive while they are registered. `LoadChunk` dereferences these
        // raw pointers when looking for an existing upvalue.
        for upv in self.upvalues.iter() {
            unsafe {
                mark_gc_object(&mut **upv);
            }
        }

        let mut new_obj_vec: Vec<Box<dyn GCObject>> = self
            .objects
            .drain(0..)
            .filter(|obj| obj.is_marked())
            .collect();
        new_obj_vec.iter_mut().for_each(|obj| obj.demark());
        self.objects = new_obj_vec;
        Ok(())
    }
    /// Value passed to this function should not
    /// be GC-managed, or memory would leak.
    pub fn load_native_module(&mut self, module_name: Option<&str>, kv: Vec<(String, Value)>) {
        if let Some(module_name) = module_name {
            let module: HashMap<IString, Value> = HashMap::from_iter(
                kv.iter()
                    .map(|(k, v)| (self.string_pool.creat_istring(&k), v.clone())),
            );
            let dict = Dict {
                marked: false,
                dict: module,
            };
            let mut managed_module = Box::new(dict);
            let p_module = managed_module.as_mut() as *mut Dict;
            self.objects.push(managed_module);
            let module_value = Value::Module(p_module);

            self.global
                .last_mut()
                .unwrap()
                .insert(self.string_pool.creat_istring(module_name), module_value);
        } else {
            // insert into current Global namespace
            for (k, v) in kv.iter() {
                self.global
                    .last_mut()
                    .unwrap()
                    .insert(self.string_pool.creat_istring(&k), v.clone());
            }
        }
    }
    pub fn load_module(&mut self, src: &str) -> EvalResult {
        let mut scanner = ScannerCtx::new(src, &mut self.string_pool);
        scanner.parse()?;
        let mut parser = ParserCtx::new(scanner.finish(), HashMap::new(), &mut self.string_pool);
        parser.parse_prog()?;
        let res = parser.finish();
        self.load_module_chunk(res.chunk)
    }

    /// Look up a module that was prepared by the AOT backend and run it in a
    /// fresh global namespace.  Returns `false` when no such module exists.
    pub fn load_precompiled_module(&mut self, path: &str) -> bool {
        let chunk = self
            .precompiled_modules
            .as_ref()
            .and_then(|m| m.get(path).cloned());
        match chunk {
            Some(chunk) => {
                let _ = self.load_module_chunk(chunk);
                true
            }
            None => false,
        }
    }

    /// Enter a module chunk in a Loader fiber, sharing the same logic as
    /// runtime source loading.
    pub fn load_module_chunk(&mut self, chunk: Chunk) -> EvalResult {
        let mut call_frames = Vec::<CallFrame>::new();
        let mut b_chunk = Box::new(chunk);
        let mut closure = Box::new(Closure {
            marked: false,
            chunk: b_chunk.as_mut() as *const Chunk,
            upvalues: Vec::new(),
            this_ref: None,
        });

        call_frames.push(CallFrame::new(
            0,
            closure.as_mut() as *mut Closure,
            Vec::new(),
        ));
        let mut fiber = Box::new(Fiber {
            marked: false,
            call_frames: call_frames,
            stack: {
                let mut vec = Vec::new();
                for _ in 0..b_chunk.num_locals {
                    vec.push(Value::Nil);
                }
                vec
            },
            state: FiberState::Loader,
            prev: self.executing_fiber,
        });
        // run module code in fresh env
        self.global.push(HashMap::new());
        self.executing_fiber = fiber.as_mut() as *mut Fiber;
        self.bump_frame_epoch();
        self.loaded_chunk.push(b_chunk);
        self.objects.push(closure);
        self.objects.push(fiber);
        Ok(())
    }

    pub fn make_managed_string(&mut self, s: &str) -> IString {
        self.string_pool.creat_istring(s)
    }

    pub fn add_object(&mut self, obj: Box<dyn GCObject>) {
        self.objects.push(obj);
    }

    pub fn get_current_fiber(&mut self) -> *mut Fiber {
        return self.executing_fiber;
    }

    pub fn set_fiber(&mut self, fiber: *mut Fiber) {
        self.executing_fiber = fiber;
        self.bump_frame_epoch();
    }

    fn get_builtin_type_extension_name(&mut self, variant: &str, name: &str) -> IString {
        let s = format!("__{variant}_{name}__");
        self.string_pool.creat_istring(&s)
    }

    pub fn get_current_glob(&mut self) -> &mut HashMap<IString, Value> {
        self.global.last_mut().unwrap()
    }

    // stack utils
    pub fn gets_number(&mut self) -> f64 {
        if let ValueKind::Number(v) = self.get_stack().pop().unwrap().kind() {
            v
        } else {
            panic!("not a Number");
        }
    }

    pub fn gets_string(&mut self) -> IString {
        if let ValueKind::String(v) = self.get_stack().pop().unwrap().kind() {
            v.clone()
        } else {
            panic!("not a Number");
        }
    }

    pub fn gets_opaque(&mut self) -> *mut u8 {
        if let ValueKind::OpaqueData(v) = self.get_stack().pop().unwrap().kind() {
            v
        } else {
            panic!("not an OpaqueData");
        }
    }
}
