use crate::*;
use std::fmt::Display;
#[derive(Debug)]
pub enum EvalError {
    Error(String),
    Exception(HashMap<String, Value>),
    ArithmError(String),
    TypeError(String),
    IndexOutOfBound(String),
    CallError(String),
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
    stack: Vec<Value>,
    call_frames: Vec<CallFrame>,
    upvalues: Vec<*mut UpValueObject>,
    objects: Vec<Box<dyn GCObject>>,
    top_chunk: Chunk,
    top_closure: Closure,
    protected: bool,
    global: Vec<Value>,
    debug: bool,
}
struct CallFrame {
    bottom: usize,
    closure: *mut Closure,
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
    pub fn new(prog: Chunk, global: Vec<Value>, debug: bool) -> Vm {
        let mut call_frames = Vec::<CallFrame>::new();
        let mut closure = Closure {
            marked: false,
            chunk: &prog as *const Chunk,
            upvalues: Vec::new(),
        };
        call_frames.push(CallFrame::new(0, &mut closure as *mut Closure));
        Vm {
            stack: {
                let mut vec = Vec::new();
                for _ in 0..prog.num_locals {
                    vec.push(Value::Nil);
                }
                vec
            },
            call_frames,
            upvalues: Vec::new(),
            objects: Vec::new(),
            top_closure: closure,
            top_chunk: prog,
            protected: false,
            global,
            debug,
        }
    }
    pub fn run(&mut self) -> EvalResult {
        loop {
            let callframe = self.call_frames.last_mut().unwrap();
            let instr = unsafe { (*(*callframe.closure).chunk).bytecodes[callframe.pc] };
            let pc = callframe.pc;
            if self.debug {
                //println!("{instr:?}");
                use std::io::{self, Write};
                println!("stack:{:?}", self.stack);
                println!(
                    "running {instr:?} in call depth {}, with pc={pc}",
                    self.call_frames.len(),
                );
                io::stdout().flush().unwrap();
            }
            match instr {
                Instr::Add => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Number(opr1 + opr2));
                    self.pc_add();
                }
                Instr::Sub => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Number(opr1 - opr2));
                    self.pc_add();
                }
                Instr::Mul => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Number(opr1 * opr2));
                    self.pc_add();
                }
                Instr::Div => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    if opr2 < 1e-5 {
                        return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                    }
                    self.stack.push(Value::Number(opr1 / opr2));
                    self.pc_add();
                }
                Instr::Mod => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    if opr2 < 1e-5 {
                        return Err(EvalError::ArithmError(self.eval_err_str("div by 0")));
                    }
                    let opr1 = opr1 as i64;
                    let opr2 = opr2 as i64;
                    self.stack.push(Value::Number((opr1 % opr2) as f64));
                    self.pc_add();
                }
                Instr::Negate => {
                    let opr = self.stack_get_number1()?;
                    self.stack.push(Value::Number(-opr));
                    self.pc_add();
                }
                Instr::Gt => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Bool(opr1 > opr2));
                    self.pc_add();
                }
                Instr::Lt => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Bool(opr1 < opr2));
                    self.pc_add();
                }
                Instr::Ge => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Bool(opr1 >= opr2));
                    self.pc_add();
                }
                Instr::Le => {
                    let (opr1, opr2) = self.stack_get_number()?;
                    self.stack.push(Value::Bool(opr1 <= opr2));
                    self.pc_add();
                }
                Instr::Eq => {
                    let opr2 = self.stack.pop().unwrap();
                    let opr1 = self.stack.pop().unwrap();
                    self.stack.push(Value::Bool(opr1 == opr2));
                    self.pc_add();
                }
                Instr::Ne => {
                    let opr2 = self.stack.pop().unwrap();
                    let opr1 = self.stack.pop().unwrap();
                    self.stack.push(Value::Bool(opr1 != opr2));
                    self.pc_add();
                }
                Instr::Or => {
                    let (opr1, opr2) = self.stack_get_bool()?;
                    self.stack.push(Value::Bool(opr1 || opr2));
                    self.pc_add();
                }
                Instr::And => {
                    let (opr1, opr2) = self.stack_get_bool()?;
                    self.stack.push(Value::Bool(opr1 && opr2));
                    self.pc_add();
                }
                Instr::PushNil => {
                    self.stack.push(Value::Nil);
                    self.pc_add();
                }
                Instr::LoadTrue => {
                    self.stack.push(Value::Bool(true));
                    self.pc_add();
                }
                Instr::LoadFalse => {
                    self.stack.push(Value::Bool(false));
                    self.pc_add();
                }
                Instr::Pop => {
                    self.stack.pop();
                    self.pc_add();
                }
                Instr::Load(x) => {
                    let v = self.get_constant(x);
                    match v {
                        Value::Chunk(chunk) => {
                            let chunk = chunk as *const Chunk;
                            //unimplemented!("for closure loading");
                            let mut upvalues = Vec::new();

                            for upval_decl in unsafe { &*chunk }.upvalues.iter() {
                                match upval_decl {
                                    UpValueDecl::Ref(idx, _) => {
                                        let current_frame_bottom =
                                            self.call_frames.last().unwrap().bottom;
                                        if let Some(x) = self.upvalues.iter().position(|p| {
                                            match unsafe { &**p } {
                                                UpValueObject {
                                                    marked: _,
                                                    value: UpValue::Ref(idx2),
                                                } => *idx2 == (*idx) + current_frame_bottom,
                                                _ => false,
                                            }
                                        }) {
                                            upvalues.push(self.upvalues[x]);
                                        } else {
                                            let upv = self
                                                .new_upvalue_object((*idx) + current_frame_bottom);
                                            upvalues.push(upv);
                                        }
                                    }
                                    UpValueDecl::RefUpValue(idx, _) => {
                                        let current_frame = self.call_frames.last().unwrap();
                                        let current_closure = unsafe { &*current_frame.closure };
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
                            self.stack.push(Value::Closure(pointer));
                        }
                        _ => {
                            let val = v.clone();
                            self.stack.push(val);
                        }
                    }
                    self.pc_add();
                }
                Instr::GetGlobal(x) => {
                    let val = self.global[x].clone();
                    self.stack.push(val);
                    self.pc_add();
                }
                Instr::GetLocal(x) => {
                    let callframe = self.call_frames.last_mut().unwrap();
                    let bottom = callframe.bottom;
                    let v = self.stack[x + bottom].clone();
                    self.stack.push(v);
                    self.pc_add();
                }
                Instr::SetLocal(x) => {
                    let callframe = self.call_frames.last_mut().unwrap();
                    let bottom = callframe.bottom;
                    let v = self.stack.last().unwrap().clone();
                    self.stack[x + bottom] = v;
                    self.pc_add();
                }
                Instr::GetUpValue(x) => {
                    let upv = self.get_upvalue(x);
                    self.stack.push(upv);
                    self.pc_add();
                }
                Instr::SetUpValue(x) => {
                    let opr = self.stack.last().unwrap().clone();
                    self.set_upvalue(x, opr);
                    self.pc_add();
                }
                Instr::InitMatrix(row, col) => {
                    self.run_gc()?;
                    let p_mat = self.new_matrix(row, col);
                    self.stack.push(Value::Matrix(p_mat));
                    self.pc_add();
                }
                Instr::InitArray(n) => {
                    self.run_gc()?;
                    let p_array = self.new_array(n);
                    self.stack.push(Value::Array(p_array));
                    self.pc_add();
                }
                Instr::InitDict(n) => {
                    self.run_gc()?;
                    let p_dict = self.new_dict(n);
                    self.stack.push(Value::Dictionary(p_dict));
                    self.pc_add();
                }
                Instr::GetCollection => {
                    let idx = self.stack.pop().unwrap();
                    let clct = self.stack.pop().unwrap();
                    let val = match clct {
                        Value::Vec2(x, y) => {
                            if let Value::Number(i) = idx {
                                let i = i as usize;
                                if i == 0 {
                                    Value::Number(x)
                                } else if i == 1 {
                                    Value::Number(y)
                                } else {
                                    return Err(EvalError::Error(
                                        self.eval_err_str("Vec2 index out of bound"),
                                    ));
                                }
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Vec2 can only be indexed by Number"),
                                ));
                            }
                        }
                        Value::Vec3(x, y, z) => {
                            if let Value::Number(i) = idx {
                                let i = i as usize;
                                if i == 0 {
                                    Value::Number(x)
                                } else if i == 1 {
                                    Value::Number(y)
                                } else if i == 2 {
                                    Value::Number(z)
                                } else {
                                    return Err(EvalError::Error(
                                        self.eval_err_str("Vec3 index out of bound"),
                                    ));
                                }
                            } else {
                                return Err(EvalError::TypeError(
                                    self.eval_err_str("Vec3 can only be indexed by Number"),
                                ));
                            }
                        }
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
                    self.stack.push(val);
                    self.pc_add();
                }
                Instr::SetCollection => {
                    let val = self.stack.pop().unwrap();
                    let idx = self.stack.pop().unwrap();
                    let clct = self.stack.pop().unwrap();
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
                    let last_pc = self.call_frames.last().unwrap().pc as i32;
                    let pc = (last_pc + x) as usize;
                    self.call_frames.last_mut().unwrap().pc = pc;
                }
                Instr::JumpIfNot(x) => {
                    if let Value::Bool(b) = self.stack.last().unwrap() {
                        if !b {
                            let last_pc = self.call_frames.last().unwrap().pc as i32;
                            let pc = (last_pc + x) as usize;
                            self.call_frames.last_mut().unwrap().pc = pc;
                        } else {
                            self.pc_add();
                        }
                        self.stack.pop();
                    } else {
                        return Err(EvalError::TypeError(
                            self.eval_err_str("condition expression must be Bool"),
                        ));
                    }
                }
                Instr::Call(x) => {
                    let val = &self.stack[self.stack.len() - x - 1];
                    if let Value::Closure(p_closure) = val {
                        let call_frame = CallFrame::new(self.stack.len() - x, *p_closure);
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
                        self.call_frames.push(call_frame);
                    } else if let Value::NativeFunction(f) = val {
                        let f = unsafe {std::mem::transmute::<*mut u8,NativeFunction>(*f)};
                        //println!("{:?}", native::sloth_print as *mut u8);
                        let v = f(&mut self.stack, x, false);
                        self.stack.pop();
                        self.stack.push(v);
                        self.pc_add();
                    } else {
                        return Err(EvalError::CallError(
                            self.eval_err_str("calling object which is not Callable"),
                        ));
                    }
                }
                Instr::TryCall(x) => {
                    let val = &self.stack[self.stack.len() - x - 1];
                    if let Value::Closure(p_closure) = val {
                        let call_frame = CallFrame::new(self.stack.len() - x, *p_closure);
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
                        self.call_frames.push(call_frame);
                    } else if let Value::NativeFunction(f) = val {
                        let f = unsafe {std::mem::transmute::<*mut u8,NativeFunction>(*f)};
                        let v = f(&mut self.stack, x, true);
                        self.stack.pop();
                        self.stack.push(v);
                        self.pc_add();
                    } else {
                        return Err(EvalError::CallError(
                            self.eval_err_str("calling object which is not Callable"),
                        ));
                    }
                }
                Instr::Except => {
                    let callframe = self.call_frames.pop().unwrap();
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
                                (**upv).value = UpValue::Closed(self.stack[idx].clone());
                            }
                        } else {
                            new_upvalues.push(*upv);
                        }
                    }
                    self.upvalues = new_upvalues;

                    if callframe.bottom + chunk.num_locals == self.stack.len() {
                        for _ in callframe.bottom..self.stack.len() {
                            self.stack.pop();
                        }
                        self.stack.pop(); //pop closure
                        let err = Value::Error(self.new_dict(0));
                        self.stack.push(err);
                    } else {
                        let val = self.stack.pop().unwrap(); // closure ret_vall <- get it
                        for _ in callframe.bottom..self.stack.len() {
                            self.stack.pop();
                        }
                        self.stack.pop(); // pop closure
                        self.stack.push(Value::String("info".to_owned()));
                        self.stack.push(val); // push return value
                        let err = Value::Error(self.new_dict(1));
                        self.stack.push(err);
                    }

                    if !self.protected || self.call_frames.is_empty() {
                        if let Value::Error(p_dict) = self.stack.pop().unwrap() {
                            let hash_map = unsafe { (*p_dict).dict.clone() };
                            return Err(EvalError::Exception(hash_map));
                        } else {
                            return Err(EvalError::Exception(HashMap::new()));
                        }
                    }
                }
                Instr::Return => {
                    let callframe = self.call_frames.pop().unwrap();
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
                                (**upv).value = UpValue::Closed(self.stack[idx].clone());
                            }
                        } else {
                            new_upvalues.push(*upv);
                        }
                    }
                    self.upvalues = new_upvalues;

                    if callframe.bottom + chunk.num_locals == self.stack.len() {
                        for _ in callframe.bottom..self.stack.len() {
                            self.stack.pop();
                        }
                        self.stack.pop(); //pop closure
                        self.stack.push(Value::Nil);
                    } else {
                        let val = self.stack.pop().unwrap(); // closure ret_vall <- get it
                        for _ in callframe.bottom..self.stack.len() {
                            self.stack.pop();
                        }
                        self.stack.pop(); // pop closure
                        self.stack.push(val); // push return value
                    }
                    if self.call_frames.is_empty() {
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
    fn new_matrix(&mut self, row: usize, col: usize) -> *mut Matrix {
        let mut ret = Box::new(Matrix {
            marked: false,
            row,
            col,
            data: {
                let mut v = Vec::new();
                v.resize_with(col * row, Default::default);
                v
            },
        });
        let pointer = ret.as_mut() as *mut Matrix;
        self.objects.push(ret);
        pointer
    }
    fn new_array(&mut self, n: usize) -> *mut Array {
        let mut vec = Vec::new();
        for _ in 0..n {
            let v = self.stack.pop().unwrap();
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
            let v = self.stack.pop().unwrap();
            let k_wrap = self.stack.pop().unwrap();
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
            self.stack.push(Value::Nil);
        }
    }
    #[inline]
    fn stack_get_number(&mut self) -> Result<(f64, f64), EvalError> {
        let (opr1, opr2);
        if let Value::Number(x) = self.stack.pop().unwrap_or(Value::Nil) {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let Value::Number(x) = self.stack.pop().unwrap_or(Value::Nil) {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_bool(&mut self) -> Result<(bool, bool), EvalError> {
        let (opr1, opr2);
        if let Value::Bool(x) = self.stack.pop().unwrap_or(Value::Nil) {
            opr2 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        if let Value::Bool(x) = self.stack.pop().unwrap_or(Value::Nil) {
            opr1 = x;
        } else {
            return Err(EvalError::TypeError(self.eval_err_str("")));
        }
        Ok((opr1, opr2))
    }
    #[inline]
    fn stack_get_number1(&mut self) -> Result<f64, EvalError> {
        if let Value::Number(x) = self.stack.pop().unwrap_or(Value::Nil) {
            Ok(x)
        } else {
            Err(EvalError::TypeError(self.eval_err_str("")))
        }
    }
    #[inline]
    fn get_constant(&mut self, idx: usize) -> &Value {
        let callframe = self.call_frames.last_mut().unwrap();
        let closure = unsafe { &*callframe.closure };
        let chunk = unsafe { &*closure.chunk };
        &chunk.constants[idx]
    }
    #[inline]
    fn get_upvalue(&mut self, idx: usize) -> Value {
        let callframe = self.call_frames.last_mut().unwrap();
        let closure = unsafe { &mut *callframe.closure };
        let upv_obj = closure.upvalues[idx];
        let upv = unsafe { &(*upv_obj).value };
        match upv {
            UpValue::Ref(idx) => self.stack[*idx].clone(),
            UpValue::Closed(value) => value.clone(),
        }
    }
    #[inline]
    fn set_upvalue(&mut self, idx: usize, v: Value) {
        let callframe = self.call_frames.last_mut().unwrap();
        let closure = unsafe { &mut *callframe.closure };
        let upv_obj = closure.upvalues[idx];
        let upv = unsafe { &mut (*upv_obj).value };
        match upv {
            UpValue::Ref(idx) => {
                self.stack[*idx] = v;
            }
            UpValue::Closed(value) => *value = v,
        }
    }
    #[inline]
    fn pc_add(&mut self) {
        let callframe = self.call_frames.last_mut().unwrap();
        callframe.nxt()
    }
    #[inline]
    fn eval_err_str(&self, s: &str) -> String {
        let callframe = self.call_frames.last().unwrap();
        let line = unsafe { (*(*callframe.closure).chunk).lines[callframe.pc] };
        format!("{s} in {line}")
    }
    fn run_gc(&mut self) -> EvalResult {
        if self.objects.len() < 128 {
            return Ok(());
        }
        for val in self.stack.iter_mut() {
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
                Value::Matrix(p_mat) => {
                    unsafe {
                        let mat = &mut **p_mat;
                        mat.mark();
                        mat.mark_children();
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
}
