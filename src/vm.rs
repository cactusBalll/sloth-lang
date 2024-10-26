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
    Exception(HashMap<String, Value>),
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

pub struct Vm {
    executing_fiber: *mut Fiber,
    upvalues: Vec<*mut UpValueObject>,
    objects: Vec<Box<dyn GCObject>>,
    // chunks: Vec<Chunk>,
    top_chunk: Box<Chunk>,     // no gc during running
    top_closure: Box<Closure>, // no gc during running
    main_fiber: Box<Fiber>,    // no gc during running
    protected: bool,
    /// should load other modules in seperated global namespace
    global: Vec<HashMap<IString, Value>>,

    /// modules other than ther main module
    loaded_chunk: Vec<Box<Chunk>>,

    string_pool: StringPool,
    debug: bool,

    pub interpreter_cwd: PathBuf,
    /// if fiber changed, pc should not be added
    pub fiber_changed: bool,
}

/// every fiber have its own stack

#[derive(Debug)]
pub struct CallFrame {
    bottom: usize,
    pub closure: *mut Closure,
    pub pc: usize,

    pub va_args: Vec<Value>,

    pub discard_return_value: bool,
}
impl CallFrame {
    pub fn new(bottom: usize, closure: *mut Closure, va_args: Vec<Value>) -> CallFrame {
        CallFrame {
            bottom,
            closure,
            pc: 0,
            va_args,
            discard_return_value: false,
        }
    }
    fn decode(&self) -> Instr {
        unsafe {
            let chunk = &*(*self.closure).chunk;
            chunk.bytecodes[self.pc]
        }
    }
    fn nxt(&mut self) {
        self.pc += 1;
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
            protected: false,
            global: vec![global],
            loaded_chunk: Vec::new(),
            string_pool,
            debug,
            interpreter_cwd,
            fiber_changed: false,
        }
    }
    pub fn get_stack<'a>(&'a self) -> &'a mut Vec<Value> {
        unsafe { &mut (*self.executing_fiber).stack }
    }

    pub fn get_call_frame<'a>(&'a self) -> &'a mut CallFrame {
        unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() }
    }
    pub fn run(&mut self) -> EvalResult {
        loop {
            let call_frame = unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() };
            let closure = call_frame.closure;
            let mut stack = unsafe { &mut (*self.executing_fiber).stack };
            let instr = unsafe { (*(*call_frame.closure).chunk).bytecodes[call_frame.pc] };
            let pc = call_frame.pc;
            if self.debug {
                dbg!(&instr);
                dbg!(&stack);
                dbg!(&call_frame);
                // dbg!(self.global.last().unwrap());
            }
            // check
            let num_locals = unsafe { (*(*call_frame.closure).chunk).num_locals };
            if stack.len() < num_locals {
                panic!(
                    "stack corrupted, VM implementation issue {} < {}",
                    stack.len(),
                    num_locals
                );
            }
            match instr {
                Instr::Add => {
                    let opr2 = stack.pop().unwrap();
                    let opr1 = stack.pop().unwrap();
                    match opr1 {
                        Value::Number(a) => {
                            if let Value::Number(b) = opr2 {
                                let res = a + b;
                                stack.push(Value::Number(res));
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("unsupported operation on `+`"),
                                ));
                            }
                            self.pc_add();
                        }
                        Value::String(a) => {
                            if let Value::String(b) = opr2 {
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
                        Value::Array(a) => {
                            if let Value::Array(b) = opr2 {
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
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(opr2);
                                        self.call_routine(1)?;
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
                    match opr1 {
                        Value::Number(a) => {
                            if let Value::Number(b) = opr2 {
                                let res = a - b;
                                stack.push(Value::Number(res));
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("unsupported operation on `-`"),
                                ));
                            }
                            self.pc_add();
                        }
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(opr2);
                                        self.call_routine(1)?;
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
                    match opr1 {
                        Value::Number(a) => {
                            if let Value::Number(b) = opr2 {
                                let res = a * b;
                                stack.push(Value::Number(res));
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("unsupported operation on `*`"),
                                ));
                            }
                            self.pc_add();
                        }
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(opr2);
                                        self.call_routine(1)?;
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
                    match opr1 {
                        Value::Number(a) => {
                            if let Value::Number(b) = opr2 {
                                if b < 1e-5 {
                                    return Err(EvalError::ArithmError(
                                        self.eval_err_str("div by 0"),
                                    ));
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
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(opr2);
                                        self.call_routine(1)?;
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
                    match opr1 {
                        Value::Number(a) => {
                            if let Value::Number(b) = opr2 {
                                if b < 1e-5 {
                                    return Err(EvalError::ArithmError(
                                        self.eval_err_str("div by 0"),
                                    ));
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
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(opr2);
                                        self.call_routine(1)?;
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
                    match opr1 {
                        Value::Number(a) => {
                            stack.push(Value::Number(-a));
                            self.pc_add();
                        }
                        Value::Instance(p_instance) => {
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
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        self.call_routine(0)?;
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
                    match opr1 {
                        Value::Bool(a) => {
                            let res = !a;
                            self.get_stack().push(Value::Bool(res));
                            self.pc_add();
                        }
                        Value::Instance(p_instance) => {
                            // not a -> a.__not__()
                            let instance = unsafe { &mut *p_instance };
                            let protocol_func_name = self.string_pool.creat_istring("__not__");
                            if let Some(_v) = instance.fields.get(&protocol_func_name) {
                                return Err(EvalError::CallError(self.eval_err_str(
                                    "`not` operation defined as field of Instance",
                                )));
                            } else {
                                if let Some(method) =
                                    unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                                {
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        self.get_stack().push(f);
                                        self.call_routine(0)?;
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
                    if let Value::Klass(klass) = stack.pop().unwrap() {
                        if let Value::Instance(instance) = stack.pop().unwrap() {
                            unsafe {
                                let mut p_klass = (*instance).klass;
                                let mut is_class = false;
                                while p_klass != null_mut() {
                                    if p_klass == klass {
                                        is_class = true;
                                        break;
                                    }
                                    p_klass = (*p_klass).super_klass;
                                }
                                stack.push(Value::Bool(is_class));
                            }
                        } else {
                            stack.push(Value::Bool(false));
                        }
                        self.pc_add();
                    } else {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("`is` can ONLY check classes, r-hand must be Class"),
                        ));
                    }
                }
                Instr::MakeRange => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Range(opr1, opr2));
                    self.pc_add();
                }
                Instr::MakeRangeClosed => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Range(opr1, opr2 + 1.));
                    self.pc_add();
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
                    let chunk = self.get_chunk(x) as *const Chunk;
                    let mut upvalues = Vec::new();

                    for upval_decl in unsafe { &*chunk }.upvalues.iter() {
                        match upval_decl {
                            UpValueDecl::Ref(idx, _) => {
                                let current_frame_bottom = call_frame.bottom;
                                if let Some(x) =
                                    self.upvalues.iter().position(|p| match unsafe { &**p } {
                                        UpValueObject {
                                            marked: _,
                                            value: UpValue::Ref(idx2),
                                        } => *idx2 == (*idx) + current_frame_bottom,
                                        _ => false,
                                    })
                                {
                                    upvalues.push(self.upvalues[x]);
                                } else {
                                    let upv =
                                        self.new_upvalue_object((*idx) + current_frame_bottom);
                                    upvalues.push(upv);
                                }
                            }
                            UpValueDecl::RefUpValue(idx, _) => {
                                let current_closure = unsafe { &*call_frame.closure };
                                let upv = current_closure.upvalues[*idx];
                                upvalues.push(upv);
                            }
                        }
                    }
                    // copy current this_ref to loaded closure
                    // for cases when lambda or functions are defined inside methods.
                    let cur_this_ref = unsafe { (*closure).this_ref.clone() };
                    let closure = Closure {
                        marked: false,
                        chunk,
                        upvalues,
                        this_ref: cur_this_ref,
                    };
                    let mut boxed_closure = Box::new(closure);
                    let pointer = boxed_closure.as_mut() as *mut Closure;
                    self.objects.push(boxed_closure);
                    stack.push(Value::Closure(pointer));
                    self.pc_add();
                }
                Instr::Load(x) => {
                    let v = self.get_constant(x);
                    let val = v.clone();
                    stack.push(val);
                    self.pc_add();
                }
                Instr::GetGlobal(x) => {
                    let call_frame = call_frame;
                    let idx = unsafe { (*(*call_frame.closure).chunk).constants[x].clone() };
                    if let Value::String(idx) = idx {
                        // dbg!(&idx);
                        let val = self.global.last().unwrap()[&idx].clone();
                        stack.push(val);
                        self.pc_add();
                    } else {
                        unreachable!();
                    }
                }
                Instr::SetGlobal(x) => {
                    let call_frame = call_frame;
                    let idx = unsafe { (*(*call_frame.closure).chunk).constants[x].clone() };
                    let v = stack.pop().unwrap();
                    if let Value::String(idx) = idx {
                        self.global.last_mut().unwrap().insert(idx, v);
                        self.pc_add();
                    } else {
                        unreachable!();
                    }
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
                    self.run_gc()?;
                    let p_array = self.new_array(n);
                    stack.push(Value::Array(p_array));
                    self.pc_add();
                }
                Instr::InitDict(n) => {
                    self.run_gc()?;
                    let p_dict = self.new_dict(n);
                    stack.push(Value::Dictionary(p_dict));
                    self.pc_add();
                }
                Instr::GetCollection(va) => {
                    let idx = stack.pop().unwrap();
                    let clct = stack.pop().unwrap();
                    match clct {
                        Value::Array(p_array) => {
                            if va == 0 {
                                if let Value::Number(i) = idx {
                                    if i < 0. {
                                        return Err(EvalError::IndexOutOfBound(self.eval_err_str(
                                            "Array cannot be indexed by negative value",
                                        )));
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
                                        self.pc_add();
                                    }
                                } else {
                                    return Err(EvalError::TypeError(
                                        self.eval_err_str("Array can only be indexed by Number"),
                                    ));
                                }
                            } else if va == 1 {
                                // `methods` on Array
                                stack.push(clct);
                                if let Value::String(s) = idx {
                                    let ext_name = self
                                        .get_builtin_type_extension_name("Array", s.get_inner());
                                    let ext_method = self.global.last().unwrap()[&ext_name].clone();
                                    stack.push(ext_method);
                                    self.pc_add();
                                } else {
                                    return Err(EvalError::TypeError(self.eval_err_str(
                                        "Array. syntax is used to call extension methods on it.",
                                    )));
                                }
                            }
                        }
                        Value::Dictionary(p_dict) => {
                            if let Value::String(s) = idx {
                                let m = unsafe { &mut *p_dict };
                                if let Some(v) = m.dict.get(&s) {
                                    stack.push(v.clone());
                                    self.pc_add();
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
                        Value::Error(p_dict) => {
                            if let Value::String(i) = idx {
                                let dict = unsafe { &mut *p_dict };
                                let elem = dict.dict.get(&i).unwrap_or(&Value::Nil).clone();
                                stack.push(elem);
                                self.pc_add();
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Error can only be indexed by String"),
                                ));
                            }
                        }
                        Value::Instance(p_instance) => {
                            if va == 0 {
                                let protocol_name = self.string_pool.creat_istring("__index__");
                                if let Some(method) =
                                    unsafe { (*(*p_instance).klass).methods.get(&protocol_name) }
                                {
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(idx);
                                        self.call_routine(1)?;
                                    } else {
                                        unreachable!()
                                    }
                                } else {
                                    return Err(EvalError::VariableNotFound(
                                        self.eval_err_str("`__index__` method not found"),
                                    ));
                                }
                            } else {
                                if let Value::String(i) = idx {
                                    let instance = unsafe { &mut *p_instance };
                                    if let Some(v) = instance.fields.get(&i) {
                                        stack.push(v.clone());
                                        self.pc_add();
                                    } else {
                                        if let Some(method) =
                                            unsafe { (*instance.klass).methods.get(&i) }
                                        {
                                            if let Value::Closure(method) = method {
                                                let mut binded_closure =
                                                    unsafe { (**method).clone() };
                                                binded_closure.this_ref = Some(p_instance);
                                                let mut b_binded_closure = Box::new(binded_closure);
                                                let p_binded_closure =
                                                    b_binded_closure.as_mut() as *mut Closure;
                                                self.objects.push(b_binded_closure);
                                                let v = Value::Closure(p_binded_closure);
                                                stack.push(v);
                                                self.pc_add();
                                            } else {
                                                unreachable!()
                                            }
                                        } else {
                                            return Err(EvalError::VariableNotFound(
                                                self.eval_err_str("method not found"),
                                            ));
                                        }
                                    }
                                } else {
                                    return Err(EvalError::TypeError(
                                        self.eval_err_str("Instance can only be indexed by String"),
                                    ));
                                }
                            }
                        }
                        Value::Module(p_module) => {
                            if let Value::String(s) = idx {
                                let m = unsafe { &mut *p_module };
                                if let Some(v) = m.dict.get(&s) {
                                    stack.push(v.clone());
                                    self.pc_add();
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
                }
                Instr::SetCollection(va) => {
                    let val = stack.pop().unwrap();
                    let idx = stack.pop().unwrap();
                    let clct = stack.pop().unwrap();
                    match clct {
                        Value::Array(p_array) => {
                            if let Value::Number(i) = idx {
                                if i < 0. {
                                    return Err(EvalError::IndexOutOfBound(self.eval_err_str(
                                        "Array cannot be indexed by negative value",
                                    )));
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
                            self.pc_add();
                        }
                        Value::Dictionary(p_dict) => {
                            if let Value::String(i) = idx {
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
                            self.pc_add();
                        }
                        Value::Error(p_dict) => {
                            if let Value::String(i) = idx {
                                let dict = unsafe { &mut *p_dict };
                                if let Some(elem) = dict.dict.get_mut(&i) {
                                    *elem = val;
                                } else {
                                    dict.dict.insert(i, val);
                                }
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Error can only be indexed by String"),
                                ));
                            }
                            self.pc_add();
                        }
                        Value::Instance(p_instance) => {
                            if va == 0 {
                                // this.__assign__(idx, val)
                                let protocol_name = self.string_pool.creat_istring("__assign__");
                                if let Some(method) =
                                    unsafe { (*(*p_instance).klass).methods.get(&protocol_name) }
                                {
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        stack.push(idx);
                                        stack.push(val);
                                        self.call_routine2(2, true)?;
                                    } else {
                                        unreachable!()
                                    }
                                } else {
                                    return Err(EvalError::VariableNotFound(
                                        self.eval_err_str("`__assign__` method not found"),
                                    ));
                                }
                            } else {
                                if let Value::String(i) = idx {
                                    let instance = unsafe { &mut *p_instance };
                                    instance.fields.insert(i.clone(), val);
                                } else {
                                    return Err(EvalError::TypeError(
                                        self.eval_err_str("Instance can only be indexed by String"),
                                    ));
                                }
                                self.pc_add();
                            }
                        }
                        v => {
                            return Err(EvalError::TypeError(self.eval_err_str(
                                format!("{:?} cannot be indexed and assigned to", v).as_ref(),
                            )));
                        }
                    }
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
                    match stack.pop().unwrap() {
                        Value::Range(l, r) => {
                            let v = Value::Range(l, r);
                            stack.push(v);
                            self.pc_add();
                        }
                        Value::String(istring) => {
                            let v = Value::StringIter(istring.clone(), 0);
                            stack.push(v);
                            self.pc_add();
                        }
                        Value::Array(array) => {
                            let v = Value::ArrayIter(array, 0);
                            stack.push(v);
                            self.pc_add();
                        }
                        Value::Dictionary(dict) => unsafe {
                            // [[k,v], [k,v], [k,v],...]
                            let kv_arr = (*dict)
                                .dict
                                .iter()
                                .map(|(k, v)| {
                                    let entry = vec![Value::String(k.clone()), v.clone()];
                                    let mut b_entry = Box::new(Array {
                                        marked: false,
                                        array: entry,
                                    });
                                    let p_entry = b_entry.as_mut() as *mut Array;
                                    self.objects.push(b_entry);
                                    Value::Array(p_entry)
                                })
                                .collect();
                            let mut b_array = Box::new(Array {
                                marked: false,
                                array: kv_arr,
                            });
                            let p_array = b_array.as_mut() as *mut Array;
                            self.objects.push(b_array);
                            let v = Value::ArrayIter(p_array, 0);
                            stack.push(v);
                            self.pc_add();
                        },
                        Value::Instance(p_instance) => {
                            let instance = unsafe { &mut *p_instance };
                            let protocol_func_name = self.string_pool.creat_istring("__iter__");
                            if let Some(v) = instance.fields.get(&protocol_func_name) {
                                let v = v.clone();
                                stack.push(v);
                                self.pc_add();
                            } else {
                                if let Some(method) =
                                    unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                                {
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        let f = Value::Closure(p_binded_closure);
                                        stack.push(f);
                                        self.call_routine(0)?;
                                    } else {
                                        unreachable!()
                                    }
                                } else {
                                    return Err(EvalError::VariableNotFound(
                                        self.eval_err_str("method not found"),
                                    ));
                                }
                            };
                        }
                        _ => {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("not subscriptable"),
                            ));
                        }
                    };
                }

                Instr::Next => match stack.last_mut().unwrap() {
                    Value::Range(l, r) => {
                        let v = if (*l - *r).abs() < f64::EPSILON {
                            Value::Nil
                        } else if l < r {
                            *l += 1.;
                            Value::Number(*l - 1.)
                        } else if l > r {
                            *l -= 1.;
                            Value::Number(*l + 1.)
                        } else {
                            unreachable!()
                        };
                        stack.push(v);
                        self.pc_add();
                    }
                    Value::ArrayIter(arr, i) => unsafe {
                        if *i >= (**arr).array.len() {
                            stack.push(Value::Nil);
                            self.pc_add();
                        } else {
                            let v = (**arr).array[*i].clone();
                            *i += 1;
                            stack.push(v);
                            self.pc_add();
                        }
                    },
                    Value::StringIter(s, i) => {
                        // performance?
                        let c = s.get_inner().chars().skip(*i).next();
                        if let Some(c) = c {
                            *i += 1;
                            let s = format!("{c}");
                            stack.push(Value::String(self.string_pool.creat_istring(&s)));
                            self.pc_add();
                        } else {
                            stack.push(Value::Nil);
                            self.pc_add();
                        }
                    }
                    Value::Instance(p_instance) => {
                        let instance = unsafe { &mut **p_instance };
                        let protocol_func_name = self.string_pool.creat_istring("__next__");
                        if let Some(v) = instance.fields.get(&protocol_func_name) {
                            let v = v.clone();
                            stack.push(v);
                            self.pc_add();
                        } else {
                            if let Some(method) =
                                unsafe { (*instance.klass).methods.get(&protocol_func_name) }
                            {
                                if let Value::Closure(method) = method {
                                    let mut binded_closure = unsafe { (**method).clone() };
                                    binded_closure.this_ref = Some(*p_instance);
                                    let mut b_binded_closure = Box::new(binded_closure);
                                    let p_binded_closure =
                                        b_binded_closure.as_mut() as *mut Closure;
                                    self.objects.push(b_binded_closure);
                                    let f = Value::Closure(p_binded_closure);
                                    stack.push(f);
                                    self.call_routine(0)?;
                                } else {
                                    unreachable!()
                                }
                            } else {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("method not found"),
                                ));
                            }
                        };
                    }
                    _ => {
                        return Err(EvalError::VariableNotFound(
                            self.eval_err_str("not a Iterator Object"),
                        ));
                    }
                },
                Instr::Call(x) => {
                    let val = &stack[stack.len() - x - 1];
                    if let Value::Closure(p_closure) = val {
                        let mut packed_va_list = Vec::new();
                        let chunk = unsafe { &*((**p_closure).chunk) };
                        if chunk.parameter_num != x {
                            if chunk.parameter_num < x && chunk.is_va {
                                for idx in (stack.len() - x + chunk.parameter_num)..stack.len() {
                                    packed_va_list.push(stack[idx].clone());
                                }
                            } else {
                                return Err(EvalError::CallError(
                                    self.eval_err_str(
                                        format!(
                                            "wrong number of argument {x}/{}",
                                            chunk.parameter_num
                                        )
                                        .as_ref(),
                                    ),
                                ));
                            }
                        }
                        let call_frame =
                            CallFrame::new(stack.len() - x, *p_closure, packed_va_list);
                        // va_list arg should be pop
                        for _ in 0..x - chunk.parameter_num {
                            stack.pop();
                        }
                        self.pc_add();
                        self.protected = false;
                        self.reserve_local(chunk.num_locals - chunk.parameter_num);
                        unsafe {
                            (*self.executing_fiber).call_frames.push(call_frame);
                        }
                    } else if let Value::Klass(klass) = val {
                        let class_idx = stack.len() - x - 1;

                        let mut b_instace = Box::new(Instance {
                            marked: false,
                            klass: *klass,
                            fields: HashMap::new(),
                        });
                        let p_instance = b_instace.as_mut() as *mut Instance;
                        self.objects.push(b_instace);
                        let idx = self.string_pool.creat_istring("__init__");
                        if let Some(method) = unsafe { (**klass).methods.get(&idx) } {
                            if let Value::Closure(method) = method {
                                let mut binded_closure = unsafe { (**method).clone() };
                                binded_closure.this_ref = Some(p_instance);
                                let mut b_binded_closure = Box::new(binded_closure);
                                let p_binded_closure = b_binded_closure.as_mut() as *mut Closure;
                                self.objects.push(b_binded_closure);
                                let init_method = Value::Closure(p_binded_closure);
                                stack[class_idx] = init_method;
                                self.call_routine(x)?;
                                // constructor evaluate to Nil
                            } else {
                                unreachable!()
                            }
                        } else {
                            // no __init__() definded
                            for _ in 0..x {
                                stack.pop();
                            }
                            stack.pop();
                            stack.push(Value::Instance(p_instance));
                            self.pc_add();
                        }
                    } else if let Value::NativeFunction(f) = val {
                        let f = unsafe { std::mem::transmute::<*mut u8, NativeFunction>(*f) };
                        //println!("{:?}", native::sloth_print as *mut u8);

                        f(self, x, false);
                        if self.fiber_changed {
                            self.fiber_changed = false;
                        } else {
                            self.pc_add();
                        }
                    } else {
                        return Err(EvalError::CallError(
                            self.eval_err_str("calling object which is not Callable"),
                        ));
                    }
                }

                Instr::Except => {
                    let callframe = unsafe { (*self.executing_fiber).call_frames.pop().unwrap() };
                    let chunk = unsafe { &*(*callframe.closure).chunk };
                    let mut new_upvalues = Vec::new();
                    for upv in self.upvalues.iter_mut() {
                        let mut escape = false;
                        let mut idx = 0;
                        if let UpValue::Ref(x) = unsafe { &(**upv).value } {
                            if *x >= callframe.bottom {
                                escape = true;
                                idx = *x;
                            }
                        }
                        if escape {
                            unsafe {
                                (**upv).value = UpValue::Closed(stack[idx].clone());
                            }
                        } else {
                            new_upvalues.push(*upv);
                        }
                    }
                    self.upvalues = new_upvalues;

                    if callframe.bottom + chunk.num_locals == stack.len() {
                        for _ in callframe.bottom..stack.len() {
                            stack.pop();
                        }
                        stack.pop(); //pop closure
                        let err = Value::Error(self.new_dict(0));
                        stack.push(err);
                    } else {
                        let val = stack.pop().unwrap(); // closure ret_vall <- get it
                        for _ in callframe.bottom..stack.len() {
                            stack.pop();
                        }
                        stack.pop(); // pop closure
                        stack.push(Value::String(self.string_pool.creat_istring("info")));
                        stack.push(val); // push return value
                        let err = Value::Error(self.new_dict(1));
                        stack.push(err);
                    }

                    if !self.protected || unsafe { (*self.executing_fiber).call_frames.is_empty() }
                    {
                        if let Value::Error(p_dict) = stack.pop().unwrap() {
                            let mut hash_map = unsafe { (*p_dict).dict.clone() };
                            return Err(EvalError::Exception(HashMap::from_iter(
                                hash_map.iter().map(|(k, v)| (k.to_string(), v.clone())),
                            )));
                        } else {
                            return Err(EvalError::Exception(HashMap::new()));
                        }
                    }
                }
                Instr::Return => {
                    let callframe = unsafe { (*self.executing_fiber).call_frames.pop().unwrap() };
                    let chunk = unsafe { &*(*callframe.closure).chunk };
                    let mut new_upvalues = Vec::new();
                    for upv in self.upvalues.iter_mut() {
                        let mut escape = false;
                        let mut idx = 0;
                        if let UpValue::Ref(x) = unsafe { &(**upv).value } {
                            if *x >= callframe.bottom {
                                escape = true;
                                idx = *x;
                            }
                        }
                        if escape {
                            unsafe {
                                (**upv).value = UpValue::Closed(stack[idx].clone());
                            }
                        } else {
                            new_upvalues.push(*upv);
                        }
                    }
                    self.upvalues = new_upvalues;

                    if callframe.bottom + chunk.num_locals == stack.len() {
                        for _ in callframe.bottom..stack.len() {
                            stack.pop();
                        }
                        stack.pop(); //pop closure
                        if !call_frame.discard_return_value {
                            stack.push(Value::Nil); // functions always return exactly ONE value, unless SetCollection
                        }
                    } else {
                        let val = stack.pop().unwrap(); // closure ret_vall <- get it
                        for _ in callframe.bottom..stack.len() {
                            stack.pop();
                        }
                        stack.pop(); // pop closure
                        if !call_frame.discard_return_value {
                            stack.push(val); // push return value
                        }
                    }
                    unsafe {
                        if (*self.executing_fiber).call_frames.is_empty() {
                            let ret_from = self.executing_fiber;
                            let prev = (*self.executing_fiber).prev;
                            if prev != null_mut() {
                                // back to prev fiber
                                self.executing_fiber = prev;
                                if (*ret_from).state == FiberState::Loader {
                                    // the instruction lead to fiber transfering should be skipped
                                    self.pc_add();
                                    let module_namespace = self.global.pop().unwrap();
                                    let mut b_module_namespace_dict = Box::new(Dict {
                                        marked: false,
                                        dict: module_namespace,
                                    });
                                    let p_module_namespace_dict =
                                        b_module_namespace_dict.as_mut() as *mut Dict;
                                    self.objects.push(b_module_namespace_dict);
                                    self.get_stack()
                                        .push(Value::Dictionary(p_module_namespace_dict));
                                } else {
                                    (*ret_from).state = FiberState::Finished;
                                    (*prev).state = FiberState::Running;
                                    self.get_stack().push(Value::Nil);
                                }
                            } else {
                                return Ok(());
                            }
                        }
                    }
                }
                Instr::GetSuperMethod => {
                    let idx = stack.pop().unwrap();
                    let clct = stack.pop().unwrap();
                    if let Value::Instance(p_instance) = clct {
                        if let Value::String(i) = idx {
                            let instance = unsafe { &mut *p_instance };
                            let mut super_class = unsafe { (*instance.klass).super_klass };
                            let mut ok = false;
                            while super_class != ptr::null_mut() {
                                if let Some(method) = unsafe { (*super_class).methods.get(&i) } {
                                    if let Value::Closure(method) = method {
                                        let mut binded_closure = unsafe { (**method).clone() };
                                        binded_closure.this_ref = Some(p_instance);
                                        let mut b_binded_closure = Box::new(binded_closure);
                                        let p_binded_closure =
                                            b_binded_closure.as_mut() as *mut Closure;
                                        self.objects.push(b_binded_closure);
                                        stack.push(Value::Closure(p_binded_closure));
                                        self.pc_add();
                                        ok = true;
                                        break;
                                    } else {
                                        super_class = unsafe { (*super_class).super_klass };
                                    }
                                }
                            }
                            if !ok {
                                return Err(EvalError::VariableNotFound(
                                    self.eval_err_str("method not found"),
                                ));
                            }
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("Instance can only be indexed by String"),
                            ));
                        }
                    } else {
                        unreachable!()
                    }
                }
                Instr::InitClass => {
                    let mut b_class = Box::new(Klass {
                        marked: false,
                        super_klass: null_mut(),
                        methods: HashMap::new(),
                    });
                    let p_class = b_class.as_mut() as *mut Klass;
                    self.objects.push(b_class);
                    stack.push(Value::Klass(p_class));
                    self.pc_add();
                }
                Instr::ClassExtend => {
                    let super_klass = stack.pop().unwrap();
                    let klass = stack.pop().unwrap();
                    if let Value::Klass(klass) = klass {
                        if let Value::Klass(super_klass) = super_klass {
                            unsafe {
                                (*klass).super_klass = super_klass;
                                // copy down methods
                                for (name, closure) in &(*super_klass).methods {
                                    (*klass).methods.insert(name.clone(), closure.clone());
                                }
                            }
                            stack.push(Value::Klass(klass));
                            self.pc_add();
                        } else {
                            return Err(EvalError::TypeError(
                                self.eval_err_str("Class can only extend Class"),
                            ));
                        }
                    } else {
                        unreachable!()
                    }
                }
                Instr::AddMethod => {
                    let method = stack.pop().unwrap();
                    let name = stack.pop().unwrap();
                    let klass = stack.pop().unwrap();
                    if let Value::Klass(klass) = klass {
                        if let Value::String(name) = name {
                            if let Value::Closure(method) = method {
                                unsafe {
                                    (*klass).methods.insert(name, Value::Closure(method));
                                }
                            } else {
                                unreachable!()
                            }
                            stack.push(Value::Klass(klass));
                            self.pc_add();
                        } else {
                            unreachable!()
                        }
                    } else {
                        unreachable!()
                    }
                }
                Instr::GetThis => {
                    let t = unsafe { (*closure).this_ref.unwrap() };
                    stack.push(Value::Instance(t));
                    self.pc_add();
                }
                Instr::Nop => {
                    self.pc_add();
                }
                Instr::UnpackVA => {
                    for elem in call_frame.va_args.iter() {
                        stack.push(elem.clone());
                    }
                    self.pc_add();
                }
                i => {
                    return Err(EvalError::Error(
                        self.eval_err_str(format!("unknown instruction {i:?}").as_ref()),
                    ))
                }
            }
        }
    }

    fn binary_predicate_impl(
        &mut self,
        op: fn(f64, f64) -> bool,
        op_str: fn(&str, &str) -> bool,
        op_name: &str,
    ) -> Result<(), EvalError> {
        let opr2 = self.get_stack().pop().unwrap();
        let opr1 = self.get_stack().pop().unwrap();
        match opr1 {
            Value::Number(a) => {
                if let Value::Number(b) = opr2 {
                    let res = op(a, b);
                    self.get_stack().push(Value::Bool(res));
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("unsupported comparing operation"),
                    ));
                }
                self.pc_add();
            }
            Value::String(s1) => {
                if let Value::String(s2) = opr2 {
                    let res = op_str(s1.get_inner(), s2.get_inner());
                    self.get_stack().push(Value::Bool(res));
                } else {
                    return Err(EvalError::TypeError(
                        self.eval_err_str("unsupported comparing operation"),
                    ));
                }
                self.pc_add();
            }
            Value::Instance(p_instance) => {
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
                        if let Value::Closure(method) = method {
                            let mut binded_closure = unsafe { (**method).clone() };
                            binded_closure.this_ref = Some(p_instance);
                            let mut b_binded_closure = Box::new(binded_closure);
                            let p_binded_closure = b_binded_closure.as_mut() as *mut Closure;
                            self.objects.push(b_binded_closure);
                            let f = Value::Closure(p_binded_closure);
                            self.get_stack().push(f);
                            self.get_stack().push(opr2);
                            self.call_routine(1)?;
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
    fn call_routine(&mut self, arg_cnt: usize) -> Result<(), EvalError> {
        self.call_routine2(arg_cnt, false)
    }
    fn call_routine2(
        &mut self,
        arg_cnt: usize,
        discard_return_value: bool,
    ) -> Result<(), EvalError> {
        let stack = self.get_stack();
        let x = arg_cnt;
        let val = &stack[stack.len() - x - 1];
        if let Value::Closure(p_closure) = val {
            let mut packed_va_list = Vec::new();
            let chunk = unsafe { &*((**p_closure).chunk) };
            if chunk.parameter_num != x {
                if chunk.parameter_num < x && chunk.is_va {
                    for idx in (stack.len() - x + chunk.parameter_num)..stack.len() {
                        packed_va_list.push(stack[idx].clone());
                    }
                } else {
                    return Err(EvalError::CallError(self.eval_err_str(
                        format!("wrong number of argument {x}/{}", chunk.parameter_num).as_ref(),
                    )));
                }
            }
            let mut call_frame = CallFrame::new(stack.len() - x, *p_closure, packed_va_list);
            call_frame.discard_return_value = discard_return_value;
            // va_list arg should be pop
            for _ in 0..x - chunk.parameter_num {
                stack.pop();
            }
            self.pc_add();
            self.protected = false;
            self.reserve_local(chunk.num_locals - chunk.parameter_num);
            unsafe {
                (*self.executing_fiber).call_frames.push(call_frame);
            }
            return Ok(());
        } else if let Value::NativeFunction(f) = val {
            let f = unsafe { std::mem::transmute::<*mut u8, NativeFunction>(*f) };
            //println!("{:?}", native::sloth_print as *mut u8);
            f(self, x, false);
            if self.fiber_changed {
                self.fiber_changed = false;
            } else {
                self.pc_add();
            }
            return Ok(());
        } else {
            return Err(EvalError::CallError(
                self.eval_err_str("calling object which is not Callable"),
            ));
        }
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
            if let Value::String(s) = k_wrap {
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
        if let Value::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil) {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let Value::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil) {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_bool(&mut self) -> Result<(bool, bool), EvalError> {
        let (opr1, opr2);
        if let Value::Bool(x) = self.get_stack().pop().unwrap_or(Value::Nil) {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let Value::Bool(x) = self.get_stack().pop().unwrap_or(Value::Nil) {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_number1(&mut self) -> Result<f64, EvalError> {
        if let Value::Number(x) = self.get_stack().pop().unwrap_or(Value::Nil) {
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
    fn get_upvalue(&mut self, idx: usize) -> Value {
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
    fn set_upvalue(&mut self, idx: usize, v: Value) {
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
        let line = unsafe { (*(*callframe.closure).chunk).lines[callframe.pc] };
        format!("{s} in {line}")
    }
    fn run_gc(&mut self) -> EvalResult {
        return Ok(());
        if self.objects.len() < 128 {
            return Ok(());
        }
        macro_rules! mark_val {
            ($v:expr) => {
                match $v {
                    Value::Array(p_arr) => {
                        unsafe {
                            let arr = &mut **p_arr;
                            arr.mark();
                            arr.mark_children();
                        };
                    }
                    Value::Dictionary(p_dict) => {
                        unsafe {
                            let dict = &mut **p_dict;
                            dict.mark();
                            dict.mark_children();
                        };
                    }
                    Value::Closure(p_closure) => {
                        unsafe {
                            let closure = &mut **p_closure;
                            closure.mark();
                            closure.mark_children();
                        };
                    }
                    Value::Fiber(p_fiber) => unsafe {
                        let fiber = &mut **p_fiber;
                        fiber.mark();
                        fiber.mark_children();
                    },
                    Value::Klass(p) => unsafe {
                        let p = &mut **p;
                        p.mark();
                        p.mark_children();
                    },
                    Value::Instance(p) => unsafe {
                        let p = &mut **p;
                        p.mark();
                        p.mark_children();
                    },
                    Value::Module(p) => unsafe {
                        let p = &mut **p;
                        p.mark();
                        p.mark_children();
                    },
                    Value::ArrayIter(p, _) => unsafe {
                        let p = &mut **p;
                        p.mark();
                        p.mark_children();
                    },
                    _ => {}
                }
            };
        }
        for val in self.main_fiber.stack.iter_mut() {
            mark_val!(val);
        }
        for g in self.global.iter_mut() {
            for (_s, v) in g.iter_mut() {
                mark_val!(v);
            }
        }
        unsafe {
            (*self.executing_fiber).mark();
            (*self.executing_fiber).mark_children();
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
        dbg!(&res.chunk);
        let mut call_frames = Vec::<CallFrame>::new();
        let mut b_chunk = Box::new(res.chunk);
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
        if let Value::Number(v) = self.get_stack().pop().unwrap() {
            v
        } else {
            panic!("not a Number");
        }
    }

    pub fn gets_string(&mut self) -> IString {
        if let Value::String(v) = self.get_stack().pop().unwrap() {
            v.clone()
        } else {
            panic!("not a Number");
        }
    }

    pub fn gets_opaque(&mut self) -> *mut u8 {
        if let Value::OpaqueData(v) = self.get_stack().pop().unwrap() {
            v
        } else {
            panic!("not an OpaqueData");
        }
    }
}
