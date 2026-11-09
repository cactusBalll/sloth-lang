//! Variable-length bytecode encoding for sloth-lang.
//!
//! The compiler still operates on [`Instr`] values because it needs a
//! convenient representation for back-patching jumps.  The representation that
//! is stored in a [`crate::Chunk`] and executed by the VM is a byte stream:
//!
//! * every instruction starts with a one byte [`OpCode`];
//! * integer operands are unsigned LEB128 encoded (so small indices take a
//!   single byte);
//! * jump operands are a fixed width signed 32 bit little endian byte offset.
//!
//! Jump offsets are measured in bytes from the start of the jump instruction
//! to the target instruction.  A fixed width jump operand is used on purpose:
//! it keeps the assembler independent of the final size of the code and makes
//! back-patching trivial while the rest of the instruction is still variable
//! length.

/// Opcode of the sloth-lang bytecode instruction set.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCode {
    Nop = 0,
    Load = 1,
    LoadChunk = 2,
    GetGlobal = 3,
    SetGlobal = 4,
    GetLocal = 5,
    SetLocal = 6,
    GetUpValue = 7,
    SetUpValue = 8,
    InitArray = 9,
    InitDict = 10,
    PushNil = 11,
    GetCollection = 12,
    SetCollection = 13,

    Add = 14,
    Sub = 15,
    Mul = 16,
    Div = 17,
    Mod = 18,
    Negate = 19,

    Not = 20,
    And = 21,
    Or = 22,

    Gt = 23,
    Lt = 24,
    Ge = 25,
    Le = 26,
    Eq = 27,
    Ne = 28,
    LoadTrue = 29,
    LoadFalse = 30,

    ClassIs = 31,

    Pop = 32,
    Swap2 = 33,
    Call = 34,
    JumpIfNot = 36,
    JumpIfTrue = 37,
    Jump = 38,
    Return = 39,

    MakeRange = 41,
    MakeRangeClosed = 42,

    Iterator = 43,
    Next = 44,

    InitClass = 45,
    AddMethod = 46,
    ClassExtend = 47,
    GetSuperMethod = 48,
    GetThis = 49,

    UnpackVA = 50,
    GetProperty = 51,
    InvokeProperty = 52,
    InvokeSuper = 53,
}

impl OpCode {
    pub fn from_u8(byte: u8) -> Option<OpCode> {
        use OpCode::*;
        Some(match byte {
            0 => Nop,
            1 => Load,
            2 => LoadChunk,
            3 => GetGlobal,
            4 => SetGlobal,
            5 => GetLocal,
            6 => SetLocal,
            7 => GetUpValue,
            8 => SetUpValue,
            9 => InitArray,
            10 => InitDict,
            11 => PushNil,
            12 => GetCollection,
            13 => SetCollection,

            14 => Add,
            15 => Sub,
            16 => Mul,
            17 => Div,
            18 => Mod,
            19 => Negate,

            20 => Not,
            21 => And,
            22 => Or,

            23 => Gt,
            24 => Lt,
            25 => Ge,
            26 => Le,
            27 => Eq,
            28 => Ne,
            29 => LoadTrue,
            30 => LoadFalse,

            31 => ClassIs,

            32 => Pop,
            33 => Swap2,
            34 => Call,
            36 => JumpIfNot,
            37 => JumpIfTrue,
            38 => Jump,
            39 => Return,

            41 => MakeRange,
            42 => MakeRangeClosed,

            43 => Iterator,
            44 => Next,

            45 => InitClass,
            46 => AddMethod,
            47 => ClassExtend,
            48 => GetSuperMethod,
            49 => GetThis,

            50 => UnpackVA,
            51 => GetProperty,
            52 => InvokeProperty,
            53 => InvokeSuper,
            _ => return None,
        })
    }
}

/// Decoded representation of an instruction.  The VM decodes one of these
/// from the byte stream before executing it.
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
    /// `collection[index]`
    GetCollection,
    /// `collection.property`
    GetProperty,
    /// `receiver.property(arg0, ..., argN-1)`
    ///
    /// Fuses `GetProperty` with the immediately following `Call`.  The name is
    /// already on the stack below the arguments; the operand is the argument
    /// count.
    InvokeProperty(usize),
    /// `super.property(arg0, ..., argN-1)`
    ///
    /// Like [`Instr::InvokeProperty`], but starts the method lookup at the
    /// receiver's superclass.
    InvokeSuper(usize),
    /// `collection[index] = value` / `collection.property = value`
    /// 0 - []
    /// 1 - .
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

    JumpIfNot(i32),
    JumpIfTrue(i32),
    Jump(i32),
    Return,

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

const JUMP_OPERAND_SIZE: usize = 4;

fn write_u64(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn read_u64(code: &[u8], pc: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut pos = pc;
    loop {
        let byte = *code.get(pos)?;
        pos += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos - pc));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn read_i32(code: &[u8], pc: usize) -> Option<(i32, usize)> {
    let bytes = code.get(pc..pc + JUMP_OPERAND_SIZE)?;
    let mut buf = [0u8; JUMP_OPERAND_SIZE];
    buf.copy_from_slice(bytes);
    Some((i32::from_le_bytes(buf), JUMP_OPERAND_SIZE))
}

fn leb128_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

fn instr_len(instr: &Instr) -> usize {
    use Instr::*;
    match instr {
        Nop | PushNil | Add | Sub | Mul | Div | Mod | Negate | Not | And | Or | Gt | Lt | Ge
        | Le | Eq | Ne | LoadTrue | LoadFalse | ClassIs | Pop | Swap2 | Return
        | MakeRange | MakeRangeClosed | Iterator | Next | InitClass | AddMethod | ClassExtend
        | GetSuperMethod | GetThis | UnpackVA | GetCollection | GetProperty => 1,
        JumpIfNot(_) | JumpIfTrue(_) | Jump(_) => 1 + JUMP_OPERAND_SIZE,
        Load(x)
        | LoadChunk(x)
        | GetGlobal(x)
        | SetGlobal(x)
        | GetLocal(x)
        | SetLocal(x)
        | GetUpValue(x)
        | SetUpValue(x)
        | InitArray(x)
        | InitDict(x)
        | SetCollection(x)
        | Call(x)
        | InvokeProperty(x)
        | InvokeSuper(x) => 1 + leb128_len(*x as u64),
    }
}

/// Encode a sequence of instructions into a variable-length byte stream.
///
/// `lines[i]` is the source line of `instrs[i]`.  The returned line vector has
/// one entry per emitted byte so that the VM can report the line of any
/// instruction by indexing the vector with the program counter.
pub fn encode_instrs(instrs: &[Instr], lines: &[usize]) -> (Vec<u8>, Vec<usize>) {
    let mut offsets = Vec::with_capacity(instrs.len() + 1);
    let mut total = 0usize;
    for instr in instrs {
        offsets.push(total);
        total += instr_len(instr);
    }
    // Allow a jump to point just past the last instruction.
    offsets.push(total);

    let mut code = Vec::with_capacity(total);
    let mut byte_lines = Vec::with_capacity(total);

    for (index, instr) in instrs.iter().enumerate() {
        let line = lines.get(index).copied().unwrap_or(0);
        let start = code.len();
        encode_one(instr, index, &offsets, &mut code);
        for _ in start..code.len() {
            byte_lines.push(line);
        }
    }

    (code, byte_lines)
}

fn encode_one(instr: &Instr, index: usize, offsets: &[usize], out: &mut Vec<u8>) {
    use Instr::*;

    fn op(out: &mut Vec<u8>, op: OpCode) {
        out.push(op as u8);
    }

    match instr {
        Nop => op(out, OpCode::Nop),
        Load(x) => {
            op(out, OpCode::Load);
            write_u64(out, *x as u64);
        }
        LoadChunk(x) => {
            op(out, OpCode::LoadChunk);
            write_u64(out, *x as u64);
        }
        GetGlobal(x) => {
            op(out, OpCode::GetGlobal);
            write_u64(out, *x as u64);
        }
        SetGlobal(x) => {
            op(out, OpCode::SetGlobal);
            write_u64(out, *x as u64);
        }
        GetLocal(x) => {
            op(out, OpCode::GetLocal);
            write_u64(out, *x as u64);
        }
        SetLocal(x) => {
            op(out, OpCode::SetLocal);
            write_u64(out, *x as u64);
        }
        GetUpValue(x) => {
            op(out, OpCode::GetUpValue);
            write_u64(out, *x as u64);
        }
        SetUpValue(x) => {
            op(out, OpCode::SetUpValue);
            write_u64(out, *x as u64);
        }
        InitArray(x) => {
            op(out, OpCode::InitArray);
            write_u64(out, *x as u64);
        }
        InitDict(x) => {
            op(out, OpCode::InitDict);
            write_u64(out, *x as u64);
        }
        PushNil => op(out, OpCode::PushNil),
        GetCollection => op(out, OpCode::GetCollection),
        GetProperty => op(out, OpCode::GetProperty),
        InvokeProperty(x) => {
            op(out, OpCode::InvokeProperty);
            write_u64(out, *x as u64);
        }
        InvokeSuper(x) => {
            op(out, OpCode::InvokeSuper);
            write_u64(out, *x as u64);
        }
        SetCollection(x) => {
            op(out, OpCode::SetCollection);
            write_u64(out, *x as u64);
        }
        Add => op(out, OpCode::Add),
        Sub => op(out, OpCode::Sub),
        Mul => op(out, OpCode::Mul),
        Div => op(out, OpCode::Div),
        Mod => op(out, OpCode::Mod),
        Negate => op(out, OpCode::Negate),
        Not => op(out, OpCode::Not),
        And => op(out, OpCode::And),
        Or => op(out, OpCode::Or),
        Gt => op(out, OpCode::Gt),
        Lt => op(out, OpCode::Lt),
        Ge => op(out, OpCode::Ge),
        Le => op(out, OpCode::Le),
        Eq => op(out, OpCode::Eq),
        Ne => op(out, OpCode::Ne),
        LoadTrue => op(out, OpCode::LoadTrue),
        LoadFalse => op(out, OpCode::LoadFalse),
        ClassIs => op(out, OpCode::ClassIs),
        Pop => op(out, OpCode::Pop),
        Swap2 => op(out, OpCode::Swap2),
        Call(x) => {
            op(out, OpCode::Call);
            write_u64(out, *x as u64);
        }
        JumpIfNot(x) => {
            op(out, OpCode::JumpIfNot);
            encode_jump(out, index, *x, offsets);
        }
        JumpIfTrue(x) => {
            op(out, OpCode::JumpIfTrue);
            encode_jump(out, index, *x, offsets);
        }
        Jump(x) => {
            op(out, OpCode::Jump);
            encode_jump(out, index, *x, offsets);
        }
        Return => op(out, OpCode::Return),
        MakeRange => op(out, OpCode::MakeRange),
        MakeRangeClosed => op(out, OpCode::MakeRangeClosed),
        Iterator => op(out, OpCode::Iterator),
        Next => op(out, OpCode::Next),
        InitClass => op(out, OpCode::InitClass),
        AddMethod => op(out, OpCode::AddMethod),
        ClassExtend => op(out, OpCode::ClassExtend),
        GetSuperMethod => op(out, OpCode::GetSuperMethod),
        GetThis => op(out, OpCode::GetThis),
        UnpackVA => op(out, OpCode::UnpackVA),
    }
}

fn encode_jump(out: &mut Vec<u8>, index: usize, instruction_offset: i32, offsets: &[usize]) {
    let target = index as isize + instruction_offset as isize;
    // A malformed program would index out of bounds here.  Clamp to the end of
    // the code instead of panicking in release builds; the VM will then run
    // off the end and report an invalid instruction.
    let target = target.max(0) as usize;
    let target = target.min(offsets.len() - 1);
    let delta = offsets[target] as isize - offsets[index] as isize;
    write_i32(out, delta as i32);
}

/// Decode one instruction at `pc`.
///
/// Returns the instruction and its encoded length on success, or the unknown
/// opcode on failure.
pub fn decode_instr(code: &[u8], pc: usize) -> Result<(Instr, usize), u8> {
    use Instr::*;

    let opcode_byte = *code.get(pc).ok_or(0)?;
    let opcode = OpCode::from_u8(opcode_byte).ok_or(opcode_byte)?;
    let operand_pc = pc + 1;

    let instr = match opcode {
        OpCode::Nop => Nop,
        OpCode::Load => Load(read_operand(code, operand_pc, pc)?),
        OpCode::LoadChunk => LoadChunk(read_operand(code, operand_pc, pc)?),
        OpCode::GetGlobal => GetGlobal(read_operand(code, operand_pc, pc)?),
        OpCode::SetGlobal => SetGlobal(read_operand(code, operand_pc, pc)?),
        OpCode::GetLocal => GetLocal(read_operand(code, operand_pc, pc)?),
        OpCode::SetLocal => SetLocal(read_operand(code, operand_pc, pc)?),
        OpCode::GetUpValue => GetUpValue(read_operand(code, operand_pc, pc)?),
        OpCode::SetUpValue => SetUpValue(read_operand(code, operand_pc, pc)?),
        OpCode::InitArray => InitArray(read_operand(code, operand_pc, pc)?),
        OpCode::InitDict => InitDict(read_operand(code, operand_pc, pc)?),
        OpCode::PushNil => PushNil,
        OpCode::GetCollection => GetCollection,
        OpCode::GetProperty => GetProperty,
        OpCode::InvokeProperty => InvokeProperty(read_operand(code, operand_pc, pc)?),
        OpCode::InvokeSuper => InvokeSuper(read_operand(code, operand_pc, pc)?),
        OpCode::SetCollection => SetCollection(read_operand(code, operand_pc, pc)?),
        OpCode::Add => Add,
        OpCode::Sub => Sub,
        OpCode::Mul => Mul,
        OpCode::Div => Div,
        OpCode::Mod => Mod,
        OpCode::Negate => Negate,
        OpCode::Not => Not,
        OpCode::And => And,
        OpCode::Or => Or,
        OpCode::Gt => Gt,
        OpCode::Lt => Lt,
        OpCode::Ge => Ge,
        OpCode::Le => Le,
        OpCode::Eq => Eq,
        OpCode::Ne => Ne,
        OpCode::LoadTrue => LoadTrue,
        OpCode::LoadFalse => LoadFalse,
        OpCode::ClassIs => ClassIs,
        OpCode::Pop => Pop,
        OpCode::Swap2 => Swap2,
        OpCode::Call => Call(read_operand(code, operand_pc, pc)?),
        OpCode::JumpIfNot => JumpIfNot(read_jump(code, operand_pc, pc)?),
        OpCode::JumpIfTrue => JumpIfTrue(read_jump(code, operand_pc, pc)?),
        OpCode::Jump => Jump(read_jump(code, operand_pc, pc)?),
        OpCode::Return => Return,
        OpCode::MakeRange => MakeRange,
        OpCode::MakeRangeClosed => MakeRangeClosed,
        OpCode::Iterator => Iterator,
        OpCode::Next => Next,
        OpCode::InitClass => InitClass,
        OpCode::AddMethod => AddMethod,
        OpCode::ClassExtend => ClassExtend,
        OpCode::GetSuperMethod => GetSuperMethod,
        OpCode::GetThis => GetThis,
        OpCode::UnpackVA => UnpackVA,
    };

    let len = instruction_len(code, pc, &instr)?;
    Ok((instr, len))
}

fn read_operand(code: &[u8], operand_pc: usize, pc: usize) -> Result<usize, u8> {
    let (value, len) = read_u64(code, operand_pc).ok_or(code[pc].wrapping_add(1))?;
    let _ = len;
    Ok(value as usize)
}

fn read_jump(code: &[u8], operand_pc: usize, pc: usize) -> Result<i32, u8> {
    let (value, _len) = read_i32(code, operand_pc).ok_or(code[pc].wrapping_add(1))?;
    Ok(value)
}

fn instruction_len(code: &[u8], pc: usize, instr: &Instr) -> Result<usize, u8> {
    let _ = instr;
    let opcode = OpCode::from_u8(code[pc]).ok_or(code[pc])?;
    let operand_pc = pc + 1;
    let len = match opcode {
        OpCode::Nop
        | OpCode::PushNil
        | OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::Mod
        | OpCode::Negate
        | OpCode::Not
        | OpCode::And
        | OpCode::Or
        | OpCode::Gt
        | OpCode::Lt
        | OpCode::Ge
        | OpCode::Le
        | OpCode::Eq
        | OpCode::Ne
        | OpCode::LoadTrue
        | OpCode::LoadFalse
        | OpCode::ClassIs
        | OpCode::Pop
        | OpCode::Swap2
        | OpCode::Return
        | OpCode::MakeRange
        | OpCode::MakeRangeClosed
        | OpCode::Iterator
        | OpCode::Next
        | OpCode::InitClass
        | OpCode::AddMethod
        | OpCode::ClassExtend
        | OpCode::GetSuperMethod
        | OpCode::GetThis
        | OpCode::UnpackVA
        | OpCode::GetCollection
        | OpCode::GetProperty => 1,
        OpCode::JumpIfNot | OpCode::JumpIfTrue | OpCode::Jump => {
            1 + JUMP_OPERAND_SIZE
        }
        _ => {
            let (_, len) = read_u64(code, operand_pc).ok_or(code[pc].wrapping_add(1))?;
            1 + len
        }
    };
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operands_are_variable_length() {
        let instrs = vec![Instr::Load(1), Instr::Load(300), Instr::PushNil];
        let (code, lines) = encode_instrs(&instrs, &[1, 2, 3]);
        assert_eq!(code.len(), 2 + 3 + 1);
        assert_eq!(lines.len(), code.len());

        let (first, first_len) = decode_instr(&code, 0).unwrap();
        assert!(matches!(first, Instr::Load(1)));
        assert_eq!(first_len, 2);

        let (second, second_len) = decode_instr(&code, first_len).unwrap();
        assert!(matches!(second, Instr::Load(300)));
        assert_eq!(second_len, 3);

        let (third, third_len) = decode_instr(&code, first_len + second_len).unwrap();
        assert!(matches!(third, Instr::PushNil));
        assert_eq!(third_len, 1);
    }

    #[test]
    fn invoke_instructions_roundtrip() {
        let instrs = vec![
            Instr::InvokeProperty(1),
            Instr::InvokeSuper(300),
            Instr::Nop,
        ];
        let (code, _) = encode_instrs(&instrs, &[0; 3]);

        let (first, first_len) = decode_instr(&code, 0).unwrap();
        assert!(matches!(first, Instr::InvokeProperty(1)));
        assert_eq!(first_len, 2);

        let (second, second_len) = decode_instr(&code, first_len).unwrap();
        assert!(matches!(second, Instr::InvokeSuper(300)));
        assert_eq!(second_len, 3);

        let (third, third_len) = decode_instr(&code, first_len + second_len).unwrap();
        assert!(matches!(third, Instr::Nop));
        assert_eq!(third_len, 1);
    }

    #[test]
    fn jumps_are_byte_offsets() {
        let instrs = vec![
            Instr::PushNil,
            Instr::Load(1),
            Instr::Jump(1), // target: instruction index 3
            Instr::Nop,
            Instr::Return,
        ];
        let (code, _) = encode_instrs(&instrs, &[0; 5]);

        let (second, second_len) = decode_instr(&code, 1).unwrap();
        assert!(matches!(second, Instr::Load(1)));
        assert_eq!(second_len, 2);

        let jump_at = 1 + second_len;
        let (jump, jump_len) = decode_instr(&code, jump_at).unwrap();
        let target = match jump {
            Instr::Jump(offset) => (jump_at as i32 + offset) as usize,
            other => panic!("expected jump, got {other:?}"),
        };
        assert_eq!(jump_len, 5);

        let (target_instr, _) = decode_instr(&code, target).unwrap();
        assert!(matches!(target_instr, Instr::Nop));
    }
}
