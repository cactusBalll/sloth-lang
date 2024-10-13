mod compiler;
#[allow(dead_code)]
mod interned_string;
mod native;
mod vm;

use std::collections::HashMap;
use std::fmt::{self, Debug};

use compiler::parser::{self, ParserCtx};
use compiler::scanner::{self, ScannerCtx};
use interned_string::{IString, StringPool};
use vm::{CallFrame, Vm};

pub fn run_string_debug(prog: &str) -> Result<(), String> {
    // let mut scanner = compiler::scanner::ScannerCtx::new(prog);
    // if let Err(e) = scanner.parse() {
    //     if e != "EOF" {
    //         return Err(e);
    //     }
    // }
    // println!("{:?}", scanner.tokens);
    // println!("{:?}", native::native_map_parser());
    // let mut parser =
    //     compiler::parser::ParserCtx::new(scanner.tokens, scanner.cood, native::native_map_parser());
    // if let Err(e) = parser.parse_prog() {
    //     if e != "EOF" {
    //         return Err(e);
    //     }
    // }
    // let chunk = parser.chunk.pop().unwrap();
    // println!("{chunk:?}");
    // let mut vm = vm::Vm::new(chunk, native::native_map_vm(), true);
    // vm.run()?;
    Ok(())
}
pub fn run_string(prog: &str) -> Result<(), String> {
    let mut string_pool = StringPool::new();
    let mut scanner = ScannerCtx::new(prog, &mut string_pool);
    scanner.parse()?;
    let scanner_result = scanner.finish();
    let mut parser = ParserCtx::new(scanner_result, HashMap::new(), &mut string_pool);
    parser.parse_prog()?;
    let parser_result = parser.finish();
    let mut vm = Box::new(Vm::new(
        parser_result.chunk,
        HashMap::new(),
        string_pool,
        true,
    ));
    vm.run()?;
    Ok(())
}

#[derive(Debug, PartialEq, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    String(IString),
    //Symbol(IString),
    Array(*mut Array),
    Dictionary(*mut Dict),
    Error(*mut Dict),
    Module(*mut Dict),
    Closure(*mut Closure),
    NativeFunction(*mut u8), //void*
    /// NativeFunctions may use it
    OpaqueData(*mut u8),
    Fiber(*mut Fiber),
}

trait GCObject {
    fn is_marked(&self) -> bool;
    fn mark(&mut self);
    fn demark(&mut self);
    fn mark_children(&mut self);
}
macro_rules! gcobject_header {
    () => {
        fn is_marked(&self) -> bool {
            self.marked
        }
        fn mark(&mut self) {
            self.marked = true;
        }
        fn demark(&mut self) {
            self.marked = false;
        }
    };
}
macro_rules! derive_gcobject {
    ($obj:ty) => {
        impl GCObject for $obj {
            gcobject_header!();
            fn mark_children(&mut self) {}
        }
    };
}
macro_rules! mark_proc {
    ($val:expr) => {
        match $val {
            Value::Array(p_arr) => {
                unsafe {
                    let arr = &mut **p_arr;
                    if !arr.is_marked() {
                        arr.mark();
                        arr.mark_children();
                    }
                };
            }
            Value::Dictionary(p_dict) => {
                unsafe {
                    let dict = &mut **p_dict;
                    if !dict.is_marked() {
                        dict.mark();
                        dict.mark_children();
                    }
                };
            }
            Value::Closure(p_closure) => {
                unsafe {
                    let closure = &mut **p_closure;
                    if !closure.is_marked() {
                        closure.mark();
                        closure.mark_children();
                    }
                };
            }

            _ => {}
        }
    };
}
derive_gcobject!(Matrix);
impl GCObject for UpValueObject {
    gcobject_header!();
    fn mark_children(&mut self) {
        match &self.value {
            UpValue::Ref(_) => {}
            UpValue::Closed(val) => {
                mark_proc!(val);
            }
        }
    }
}

impl GCObject for Array {
    gcobject_header!();
    fn mark_children(&mut self) {
        for val in self.array.iter() {
            mark_proc!(val);
        }
    }
}
impl GCObject for Closure {
    gcobject_header!();
    fn mark_children(&mut self) {
        for p_upv in self.upvalues.iter() {
            unsafe {
                let upv = *p_upv;
                if !(*upv).is_marked() {
                    (*upv).mark();
                    (*upv).mark_children();
                }
            }
        }
    }
}
impl GCObject for Dict {
    gcobject_header!();
    fn mark_children(&mut self) {
        for val in self.dict.values() {
            mark_proc!(val);
        }
    }
}

impl GCObject for Fiber {
    gcobject_header!();
    fn mark_children(&mut self) {
        for call_frame in self.call_frames.iter_mut() {
            unsafe {
                let closure = &mut (*call_frame.closure);
                if !closure.is_marked() {
                    closure.mark();
                    closure.mark_children();
                }
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
                    _ => {}
                }
            }
        }
    }
}
#[derive(Debug)]
pub struct Array {
    pub marked: bool,
    pub array: Vec<Value>,
}
#[derive(Debug)]
pub struct Dict {
    pub marked: bool,
    pub dict: HashMap<IString, Value>,
}
#[derive(Debug)]
pub enum FiberState {
    /// not executed yet
    Initial,
    /// fiber running
    Running,
    /// yield or transfer called during execution
    Paused,
    /// error occured
    Error,
    /// finished Fiber should not be resumed
    Finished,
}
#[derive(Debug)]
pub struct Fiber {
    pub marked: bool,
    pub call_frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub state: FiberState,
    pub prev: *mut Fiber,
}

#[derive(Debug, Clone)]
pub enum UpValueDecl {
    Ref(usize, IString),
    RefUpValue(usize, IString),
}
#[derive(Debug)]
pub enum UpValue {
    Closed(Value),
    Ref(usize),
}
#[derive(Debug)]
pub struct UpValueObject {
    pub marked: bool,
    pub value: UpValue,
}
#[derive(Debug)]
pub struct Matrix {
    pub marked: bool,
    pub row: usize,
    pub col: usize,
    pub data: Vec<f64>,
}
#[derive(Default, Clone)]
pub struct Chunk {
    pub bytecodes: Vec<Instr>,
    pub lines: Vec<usize>,
    pub constants: Vec<Value>,
    pub chunks: Vec<Chunk>,
    pub file: String,
    pub upvalues: Vec<UpValueDecl>,
    pub parameter_num: usize,
    pub num_locals: usize,
}
impl PartialEq for Chunk {
    fn eq(&self, _other: &Self) -> bool {
        false //it is never necessary to compare two Chunk
    }
}
impl Debug for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        for i in 0..(self.bytecodes.len()) {
            writeln!(f, "{:>15}   {:?}", self.lines[i], self.bytecodes[i])?;
        }
        writeln!(f, "constants: {:?}", self.constants)?;
        writeln!(f, "upvalues: {:?}", self.upvalues)?;
        writeln!(f)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub marked: bool,
    pub chunk: *const Chunk,
    pub upvalues: Vec<*mut UpValueObject>,
}
impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.chunk == other.chunk
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Instr {
    Load(usize),
    LoadChunk(usize),
    GetGlobal(usize),
    SetGlobal(usize),
    GetLocal(usize),
    SetLocal(usize),
    GetUpValue(usize),
    SetUpValue(usize),
    InitArray(usize), /*size of array*/
    InitDict(usize),
    PushNil,

    GetCollection,
    SetCollection,

    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Negate,

    Not,
    And,
    Or,

    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
    LoadTrue,
    LoadFalse,

    Pop,
    Call(usize), /*parameter num*/
    TryCall(usize),
    JumpIfNot(i32),
    Jump(i32),
    Return,
    Except,

    Concat,
}
type NativeFunction = fn(&mut Vec<Value>, usize, bool) -> Value;
#[cfg(test)]
mod test {
    #[test]
    fn test0() {
        assert!('\t'.is_whitespace());
        let mut b = Box::new(666);
        assert_eq!(b.as_mut() as *mut usize, Box::into_raw(b));
    }
}
