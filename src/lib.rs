mod compiler;
mod extension_methods;
mod fiber;
#[allow(dead_code)]
mod interned_string;
mod native;
mod vec;
mod vm;

use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::io::Write;

use compiler::parser::{self, ParserCtx};
use compiler::scanner::{self, ScannerCtx};
use interned_string::{IString, StringPool};
use native::{
    sloth_input, sloth_load_module, sloth_print_val, sloth_to_bool, sloth_to_number,
    sloth_to_string, sloth_typeof, sloth_va_arg,
};
use vm::{CallFrame, Vm};

macro_rules! mf_entry {
    ($name:expr,$func:expr) => {
        ($name.to_owned(), Value::NativeFunction($func as *mut u8))
    };
}
pub fn prelude() -> Vec<(String, Value)> {
    vec![
        (
            "print".to_owned(),
            Value::NativeFunction(sloth_print_val as *mut u8),
        ),
        (
            "import".to_owned(),
            Value::NativeFunction(sloth_load_module as *mut u8),
        ),
        (
            "number".to_owned(),
            Value::NativeFunction(sloth_to_number as *mut u8),
        ),
        (
            "string".to_owned(),
            Value::NativeFunction(sloth_to_string as *mut u8),
        ),
        (
            "bool".to_owned(),
            Value::NativeFunction(sloth_to_bool as *mut u8),
        ),
        (
            "input".to_owned(),
            Value::NativeFunction(sloth_input as *mut u8),
        ),
        (
            "type_string".to_owned(),
            Value::NativeFunction(sloth_typeof as *mut u8),
        ),
        (
            "va_arg".to_owned(),
            Value::NativeFunction(sloth_va_arg as *mut u8),
        ),
        mf_entry!("__Array_push__", extension_methods::array_push),
    ]
}
pub fn run_string(prog: &str, only_compile: bool) -> Result<(), String> {
    let mut string_pool = StringPool::new();
    let mut scanner = ScannerCtx::new(prog, &mut string_pool);
    scanner.parse()?;
    let scanner_result = scanner.finish();
    println!("{:?}", scanner_result.tokens);
    let mut parser = ParserCtx::new(scanner_result, HashMap::new(), &mut string_pool);
    parser.parse_prog()?;
    let parser_result = parser.finish();
    println!("{:?}", parser_result.chunk);
    let _ = std::io::stdout().flush();
    let cwd = std::env::current_dir().unwrap();
    println!("interpreter running in {cwd:?}");
    let mut vm = Box::new(Vm::new(
        parser_result.chunk,
        HashMap::new(),
        string_pool,
        true,
        cwd,
    ));
    vm.load_native_module(None, prelude());
    let (name, module) = fiber::module_export();
    vm.load_native_module(Some(&name), module);
    if !only_compile {
        vm.run()?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Range(f64, f64),
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

    Klass(*mut Klass),
    Instance(*mut Instance),

    StringIter(IString, usize),
    ArrayIter(*mut Array, usize),
}

impl Value {
    pub fn to_bool_v(&self) -> Value {
        Value::Bool(self.to_bool())
    }
    pub fn to_bool(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(v) => *v,
            _ => true,
        }
    }
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
        unsafe {
            if let Some(has_this_ref) = self.this_ref {
                if !(*has_this_ref).is_marked() {
                    (*has_this_ref).mark();
                    (*has_this_ref).mark_children();
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

impl GCObject for Klass {
    gcobject_header!();
    fn mark_children(&mut self) {
        unsafe {
            if !(*self.super_klass).is_marked() {
                (*self.super_klass).mark();
                (*self.super_klass).mark_children();
            }
        }
        for val in self.methods.values() {
            mark_proc!(val);
        }
    }
}

impl GCObject for Instance {
    gcobject_header!();
    fn mark_children(&mut self) {
        unsafe {
            if !(*self.klass).is_marked() {
                (*self.klass).mark();
                (*self.klass).mark_children();
            }
        }
        for val in self.fields.values() {
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
#[derive(Debug, PartialEq)]
pub enum FiberState {
    /// not executed yet
    Initial,
    /// this fiber call resume
    Waiting,
    /// running
    Running,
    /// yield or transfer called during execution
    Paused,
    /// error occured
    Error,
    /// finished Fiber should not be resumed
    Finished,
    /// loading module
    Loader,
}
#[derive(Debug)]
pub struct Fiber {
    pub marked: bool,
    pub call_frames: Vec<CallFrame>,
    pub stack: Vec<Value>,
    pub state: FiberState,
    pub prev: *mut Fiber,
}

#[derive(Debug)]
pub struct Klass {
    pub marked: bool,
    pub super_klass: *mut Klass,
    pub methods: HashMap<IString, Value>,
}

#[derive(Debug)]
pub struct Instance {
    pub marked: bool,
    pub klass: *mut Klass,
    pub fields: HashMap<IString, Value>,
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
    pub is_va: bool,
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
        writeln!(f, "chunks: {:?}", self.chunks)?;
        writeln!(f)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub marked: bool,
    pub chunk: *const Chunk,
    pub upvalues: Vec<*mut UpValueObject>,
    pub this_ref: Option<*mut Instance>,
}
impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.chunk == other.chunk
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Instr {
    Nop,

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
    /// . / [] have different semmantic on Instance of Classes
    /// due to the involvment of operator overriding.
    /// 0 - []
    /// 1 - .
    GetCollection(usize),
    SetCollection(usize),

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

    ClassIs,

    Pop,
    Swap2,       /*change top 2 value on the stack*/
    Call(usize), /*parameter num*/

    TryCall(usize),
    JumpIfNot(i32),
    JumpIfTrue(i32),
    Jump(i32),
    Return,
    Except,

    MakeRange,
    MakeRangeClosed,

    Iterator,
    Next,

    InitClass,
    AddMethod,
    ClassExtend,
    GetSuperMethod,
    GetThis,

    UnpackVA,
}
type NativeFunction = fn(&mut Vm, usize, bool);
#[cfg(test)]
mod test {
    use super::run_string;
    #[test]
    fn pipe_test() {
        let src = r#"
            func mul2(a) {
                return a * 2;
            }
            print(2 |> mul2 |> mul2 |> mul2);
        "#;
        let res = run_string(&src, false);
        println!("{:?}", res);
    }
    #[test]
    fn class_test() {
        let src = r#"
            class Fish{}
            class Mammal{
                func __init__() {
                    this.weight = 100;
                    this.height = 100;
                }
                func say() {
                    print("Mammal", this.weight, this.height);
                }
            }

            class Cat:Mammal{
                func __init__() {
                    super.__init__();
                    this.height = 70;
                }
            }

            var cat = Cat();
            cat.say();
            print(cat is Cat, cat is Mammal, cat is Fish);
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn container_test() {
        let src = r#"
            var a = @("a":3, "b": [1,2,3]);
            print(a["a"], a.b[1]);
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn short_circuit_test() {
        let src = r#"
            (1 < 2) or ||{print("not executed");}();
            (1 < 2) and ||{print("executed");}();
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn lambda_in_method() {
        let src = r#"
            class Foo{
                func __init__() {
                    this.age = 24;
                }
                func des() {
                    return ||{
                        this.age = this.age + 1;
                        print(this.age);
                    };
                }
            }
            var foo = Foo();
            var des = foo.des();
            des();
            des();
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn loop1() {
        let src = r#"
        var a = 3;
        while (true) {
            a = a - 1;
            if (a <= 0) {
                break;
            }
        }
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn loop2() {
        let src = r#"
        var a = 10;
        while (true) {
            a = a - 1;
            if (a <= 0) {
                break;
            }
            var b = 10;
            while(true) {
                b = b - 1;
                if (b <= 0) {
                    break;
                }
                print(b, " ");
            }
        }
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn load_module() {
        let src = r#"
            var m = import("test_module.slt");
            var counter = m.Counter();
            print(m.PI);
            counter.inc_by(100);
            print(counter.value);
            counter.dec_by(50);
            print(counter.value);
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn for_loop() {
        let src = r#"
            for (var i: 1..10) {
                print(i);
            }
            print("\n");
            for (var i: 1..=10) {
                print(i);
            }
            print("\n");
            for (var c: "hello world") {
                print(c);
            }
            print("\n");
            for (var i: [1,1,4,5,1,4]) {
                print(i);
            }
            print("\n");
            for (var i: @("msvc":2, "clang":3, "gcc":5, "icc":9, "emscripten": 7)) {
                print(i[0], i[1]);
            }
            print("\n");
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn iterator_protocol() {
        let src = r#"
            class Foo{
                func __iter__() {
                    class Iter{
                        func __init__() {
                            this.x = 1;
                        }
                        func __next__() {
                            this.x = this.x + 1;
                            if (this.x < 20) {
                                return this.x;
                            } else {
                                return nil;
                            }
                        }
                    }
                    return Iter();
                }
            }
            var foo = Foo();
            for (var i: foo) {
                print(i);
            }
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn operator_override() {
        let src = r#"
            class Point{
                func __init__(x,y) {
                    this.x = x;
                    this.y = y;
                }
                func get_length() {
                    return this.x * this.x + this.y * this.y;
                }
                func __add__(rhs) {
                    return Point(this.x + rhs.x, this.y + rhs.y);
                }
                func __sub__(rhs) {
                    return Point(this.x - rhs.x, this.y - rhs.y);
                }
                func __eq__(rhs) {
                    return this.x == rhs.x and this.y == rhs.y;
                }
                func __ne__(rhs) {
                    return not this.__eq__(rhs);
                }
            }

            var a = Point(2,3);
            var b = Point(1,4);
            print(a == b, a != b);
            var c = a + b;
            var d = a - b;
            print(c.x, c.y);
            print(d.x, d.y);
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn operator_override2() {
        let src = r#"
            class Vec{
                func __init__(arr) {
                    this.arr = arr;
                }
                func set_vec(...) {
                    this.arr = va_arg();
                }
                func __assign__(idx, val) {
                    this.arr[idx] = val;
                }
                func __index__(idx) {
                    return this.arr[idx];
                }
            }

            var vec = Vec([1,3,2,4]);
            print(vec.arr);
            vec[2] = 10;
            vec.__assign__(3,100);
            print(vec.arr);
            print(vec[2]);
            vec.set_vec(1,1,4,5,1,4);
            print(vec.arr);
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn fiber0() {
        let src = r#"
            var f = fiber.create(|n|{
                var i = 0;
                while(i < n) {
                    var v = fiber.yield(i);
                    print(v);
                    i = i + 1;
                }
            },10);
            print(fiber.resume(f));
            print(fiber.resume(f, "hello fiber"));
            print(fiber.resume(f, "good fiber"));
            print(fiber.resume(f, "bad fiber"));
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }
    #[test]
    fn fiber1() {
        let src = r#"
            var f = fiber.create(|n|{
                var i = 0;
                while(i < n) {
                    var v = fiber.yield(i);
                    print(v);
                    i = i + 1;
                    print(i);
                    if (i > 5) {
                        print("error in fiber");
                        fiber.error();
                    }
                }
            },10);
            
            while(fiber.check(f)) {
                print(fiber.resumable(f));
                fiber.resume(f);
            }
        "#;
        let res = run_string(&src, false);
        println!("{res:?}");
    }

    #[test]
    fn minimal() {
        let src = r#"
            print("233")
            print("114514");
        "#;
        let res = run_string(&src, true);
        println!("{res:?}");
    }
}
