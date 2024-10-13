use interned_string::{IString, StringPool};

use crate::*;
use std::{fmt::Display, ptr::null_mut};
#[derive(Debug)]
pub enum EvalError {
    Error(String),
    Exception(HashMap<String, Value>),
    ArithmError(String),
    TypeError(String),
    IndexOutOfBound(String),
    CallError(String),
    VariableNotFound(String),
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
type EvalResult = Result<(), EvalError>;

pub struct Vm {
    executing_fiber: *mut Fiber,
    upvalues: Vec<*mut UpValueObject>,
    objects: Vec<Box<dyn GCObject>>,
    // chunks: Vec<Chunk>,
    top_chunk: Chunk,     // no gc during running
    top_closure: Closure, // no gc during running
    main_fiber: Fiber,    // no gc during running
    protected: bool,
    global: HashMap<IString, Value>,

    string_pool: StringPool,
    debug: bool,
}

/// every fiber have its own stack

#[derive(Debug)]
pub struct CallFrame {
    bottom: usize,
    pub closure: *mut Closure,
    pc: usize,
}
impl CallFrame {
    fn new(bottom: usize, closure: *mut Closure) -> CallFrame {
        CallFrame {
            bottom,
            closure,
            pc: 0,
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
    ) -> Vm {
        let mut call_frames = Vec::<CallFrame>::new();
        let mut closure = Closure {
            marked: false,
            chunk: &prog as *const Chunk,
            upvalues: Vec::new(),
        };
        call_frames.push(CallFrame::new(0, &mut closure as *mut Closure));
        let mut fiber = Fiber {
            marked: false,
            call_frames: call_frames,
            stack: {
                let mut vec = Vec::new();
                for _ in 0..prog.num_locals {
                    vec.push(Value::Nil);
                }
                vec
            },
            state: FiberState::Running,
            prev: null_mut() as *mut Fiber,
        };

        Vm {
            executing_fiber: &mut fiber as *mut Fiber,
            upvalues: Vec::new(),
            objects: Vec::new(),
            top_closure: closure,
            top_chunk: prog,
            main_fiber: fiber,
            protected: false,
            global,
            string_pool,
            debug,
        }
    }
    fn get_stack<'a>(&'a self) -> &'a mut Vec<Value> {
        unsafe { &mut (*self.executing_fiber).stack }
    }

    fn get_call_frame<'a>(&'a self) -> &'a mut CallFrame {
        unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() }
    }
    pub fn run(&mut self) -> EvalResult {
        loop {
            let call_frame = unsafe { (*self.executing_fiber).call_frames.last_mut().unwrap() };
            let mut stack = unsafe { &mut (*self.executing_fiber).stack };
            let instr = unsafe { (*(*call_frame.closure).chunk).bytecodes[call_frame.pc] };
            let pc = call_frame.pc;
            if self.debug {}
            match instr {
                Instr::Add => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Number(opr1 + opr2));
                    self.pc_add();
                }
                Instr::Sub => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Number(opr1 - opr2));
                    self.pc_add();
                }
                Instr::Mul => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Number(opr1 * opr2));
                    self.pc_add();
                }
                Instr::Div => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    if opr2 < 1e-5 {
                        return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                    }
                    stack.push(Value::Number(opr1 / opr2));
                    self.pc_add();
                }
                Instr::Mod => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    if opr2 < 1e-5 {
                        return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                    }
                    let opr1 = opr1 as i64;
                    let opr2 = opr2 as i64;
                    stack.push(Value::Number((opr1 % opr2) as f64));
                    self.pc_add();
                }
                Instr::Negate => {
                    let opr = self.stack_get_number1()?;
                    stack.push(Value::Number(-opr));
                    self.pc_add();
                }
                Instr::Gt => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Bool(opr1 > opr2));
                    self.pc_add();
                }
                Instr::Lt => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Bool(opr1 < opr2));
                    self.pc_add();
                }
                Instr::Ge => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Bool(opr1 >= opr2));
                    self.pc_add();
                }
                Instr::Le => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    stack.push(Value::Bool(opr1 <= opr2));
                    self.pc_add();
                }
                Instr::Eq => {
                    let opr2 = stack.pop().unwrap();
                    let opr1 = stack.pop().unwrap();
                    stack.push(Value::Bool(opr1 == opr2));
                    self.pc_add();
                }
                Instr::Ne => {
                    let opr2 = stack.pop().unwrap();
                    let opr1 = stack.pop().unwrap();
                    stack.push(Value::Bool(opr1 != opr2));
                    self.pc_add();
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
                Instr::LoadChunk(x) => {
                    let chunk = self.get_chunk(x) as *const Chunk;
                    //unimplemented!("for closure loading");
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
                    let closure = Closure {
                        marked: false,
                        chunk,
                        upvalues,
                    };
                    let mut boxed_closure = Box::new(closure);
                    let pointer = boxed_closure.as_mut() as *mut Closure;
                    self.objects.push(boxed_closure);
                    stack.push(Value::Closure(pointer));
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
                        let val = self.global[&idx].clone();
                        stack.push(val);
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
                    let v = stack.last().unwrap().clone();
                    stack[x + bottom] = v;
                    self.pc_add();
                }
                Instr::GetUpValue(x) => {
                    let upv = self.get_upvalue(x);
                    stack.push(upv);
                    self.pc_add();
                }
                Instr::SetUpValue(x) => {
                    let opr = stack.last().unwrap().clone();
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
                Instr::GetCollection => {
                    let idx = stack.pop().unwrap();
                    let clct = stack.pop().unwrap();
                    let val = match clct {
                        Value::Array(p_array) => {
                            if let Value::Number(i) = idx {
                                if i < 0. {
                                    return Err(EvalError::IndexOutOfBound(self.eval_err_str(
                                        "Array cannot be indexed by negative value",
                                    )));
                                }
                                let i = i as usize;
                                let arr = unsafe { &mut *p_array };
                                arr.array.get(i).unwrap_or(&Value::Nil).clone()
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Array can only be indexed by Number"),
                                ));
                            }
                        }
                        Value::Dictionary(p_dict) => {
                            if let Value::String(i) = idx {
                                let dict = unsafe { &mut *p_dict };
                                dict.dict.get(&i).unwrap_or(&Value::Nil).clone()
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Dict can only be indexed by String"),
                                ));
                            }
                        }
                        Value::Error(p_dict) => {
                            if let Value::String(i) = idx {
                                let dict = unsafe { &mut *p_dict };
                                dict.dict.get(&i).unwrap_or(&Value::Nil).clone()
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Error can only be indexed by String"),
                                ));
                            }
                        }
                        v => {
                            return Err(EvalError::TypeError(
                                self.eval_err_str(format!("{:?} can not be indexed", v).as_ref()),
                            ));
                        }
                    };
                    stack.push(val);
                    self.pc_add();
                }
                Instr::SetCollection => {
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
                        }
                        v => {
                            return Err(EvalError::TypeError(self.eval_err_str(
                                format!("{:?} cannot be indexed and assigned to", v).as_ref(),
                            )));
                        }
                    }
                    self.pc_add();
                }
                Instr::Jump(x) => {
                    let last_pc = call_frame.pc as i32;
                    let pc = (last_pc + x) as usize;
                    call_frame.pc = pc;
                }
                Instr::JumpIfNot(x) => {
                    if let Value::Bool(b) = stack.last().unwrap() {
                        if !b {
                            let last_pc = call_frame.pc as i32;
                            let pc = (last_pc + x) as usize;
                            call_frame.pc = pc;
                        } else {
                            self.pc_add();
                        }
                        stack.pop();
                    } else {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("condition expression must be Bool"),
                        ));
                    }
                }
                Instr::Call(x) => {
                    let val = &stack[stack.len() - x - 1];
                    if let Value::Closure(p_closure) = val {
                        let call_frame = CallFrame::new(stack.len() - x, *p_closure);
                        let chunk = unsafe { &*((**p_closure).chunk) };
                        if chunk.parameter_num != x {
                            return Err(EvalError::CallError(
                                self.eval_err_str(
                                    format!("wrong number of argument {x}/{}", chunk.parameter_num)
                                        .as_ref(),
                                ),
                            ));
                        }
                        self.pc_add();
                        self.protected = false;
                        self.reserve_local(chunk.num_locals - chunk.parameter_num);
                        unsafe {
                            (*self.executing_fiber).call_frames.push(call_frame);
                        }
                    } else if let Value::NativeFunction(f) = val {
                        let f = unsafe { std::mem::transmute::<*mut u8, NativeFunction>(*f) };
                        //println!("{:?}", native::sloth_print as *mut u8);
                        let v = f(&mut stack, x, false);
                        stack.pop();
                        stack.push(v);
                        self.pc_add();
                    } else {
                        return Err(EvalError::CallError(
                            self.eval_err_str("calling object which is not Callable"),
                        ));
                    }
                }
                Instr::TryCall(x) => {
                    let val = &stack[stack.len() - x - 1];
                    if let Value::Closure(p_closure) = val {
                        let call_frame = CallFrame::new(stack.len() - x, *p_closure);
                        let chunk = unsafe { &*((**p_closure).chunk) };
                        if chunk.parameter_num != x {
                            return Err(EvalError::CallError(
                                self.eval_err_str(
                                    format!("wrong number of argument {x}/{}", chunk.parameter_num)
                                        .as_ref(),
                                ),
                            ));
                        }
                        self.pc_add();
                        self.protected = true;
                        self.reserve_local(chunk.num_locals - chunk.parameter_num);
                        unsafe {
                            (*self.executing_fiber).call_frames.push(call_frame);
                        }
                    } else if let Value::NativeFunction(f) = val {
                        let f = unsafe { std::mem::transmute::<*mut u8, NativeFunction>(*f) };
                        let v = f(&mut stack, x, true);
                        stack.pop();
                        stack.push(v);
                        self.pc_add();
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
                        stack.push(Value::Nil);
                    } else {
                        let val = stack.pop().unwrap(); // closure ret_vall <- get it
                        for _ in callframe.bottom..stack.len() {
                            stack.pop();
                        }
                        stack.pop(); // pop closure
                        stack.push(val); // push return value
                    }
                    if unsafe { (*self.executing_fiber).call_frames.is_empty() } {
                        return Ok(());
                    }
                }
                i => {
                    return Err(EvalError::Error(
                        self.eval_err_str(format!("unknown instruction {i:?}").as_ref()),
                    ))
                }
            }
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
        if self.objects.len() < 128 {
            return Ok(());
        }
        for val in self.main_fiber.stack.iter_mut() {
            match val {
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
                _ => {}
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
    pub fn load_native_module(&mut self, module_name: &str, kv: Vec<(String, Value)>) {
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
            .insert(self.string_pool.creat_istring(module_name), module_value);
    }
}
