//! Serialized program image used by the AOT backend.
//!
//! The image contains the complete [`Chunk`] tree of the main program and of
//! every statically imported module.  The AOT IR embeds the image as a byte
//! array; the runtime deserializes it during startup and never reads the
//! original source files again.

use crate::interned_string::StringPool;
use crate::{Chunk, UpValueDecl, Value, ValueKind};

const MAGIC: &[u8; 4] = b"SLTI";
const VERSION: u32 = 1;

/// One root chunk: the main program (empty path) or an imported module.
#[derive(Debug, Clone)]
pub struct ImageRoot {
    pub path: String,
    pub chunk: Chunk,
}

/// Serialize the main chunk and all imported modules.
pub fn serialize(roots: &[ImageRoot]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_u32(&mut out, VERSION);
    write_u32(&mut out, roots.len() as u32);
    for root in roots {
        write_string(&mut out, &root.path)?;
        write_chunk(&mut out, &root.chunk)?;
    }
    Ok(out)
}

/// Deserialize an image.  String constants are interned into `pool`.
pub fn deserialize(bytes: &[u8], pool: &mut StringPool) -> Result<Vec<ImageRoot>, String> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != MAGIC {
        return Err("invalid SLTI image magic".to_string());
    }
    let version = cursor.read_u32()?;
    if version != VERSION {
        return Err(format!("unsupported SLTI image version {version}"));
    }
    let root_count = cursor.read_u32()? as usize;
    let mut roots = Vec::with_capacity(root_count);
    for _ in 0..root_count {
        let path = cursor.read_string()?;
        let chunk = read_chunk(&mut cursor, pool)?;
        roots.push(ImageRoot { path, chunk });
    }
    if !cursor.is_empty() {
        return Err("trailing bytes in SLTI image".to_string());
    }
    Ok(roots)
}

fn write_chunk(out: &mut Vec<u8>, chunk: &Chunk) -> Result<(), String> {
    write_u32(out, chunk.native_id as u32);
    write_string(out, &chunk.file)?;
    write_u32(out, chunk.parameter_num as u32);
    write_u32(out, chunk.num_locals as u32);
    write_u8(out, chunk.is_va as u8);

    write_u32(out, chunk.upvalues.len() as u32);
    for upvalue in &chunk.upvalues {
        match upvalue {
            UpValueDecl::Ref(index, name) => {
                write_u8(out, 0);
                write_u32(out, *index as u32);
                write_string(out, name.get_inner())?;
            }
            UpValueDecl::RefUpValue(index, name) => {
                write_u8(out, 1);
                write_u32(out, *index as u32);
                write_string(out, name.get_inner())?;
            }
        }
    }

    write_u32(out, chunk.constants.len() as u32);
    for constant in &chunk.constants {
        match constant.kind() {
            ValueKind::Number(number) => {
                write_u8(out, 0);
                write_u64(out, number.to_bits());
            }
            ValueKind::String(string) => {
                write_u8(out, 1);
                write_string(out, string.get_inner())?;
            }
            other => {
                return Err(format!("unsupported constant in AOT image: {other:?}"));
            }
        }
    }

    write_u32(out, chunk.bytecodes.len() as u32);
    out.extend_from_slice(&chunk.bytecodes);
    write_u32(out, chunk.lines.len() as u32);
    for line in &chunk.lines {
        write_u32(out, *line as u32);
    }

    write_u32(out, chunk.chunks.len() as u32);
    for child in &chunk.chunks {
        write_chunk(out, child)?;
    }
    Ok(())
}

fn read_chunk(cursor: &mut Cursor<'_>, pool: &mut StringPool) -> Result<Chunk, String> {
    let native_id = cursor.read_u32()? as usize;
    let file = cursor.read_string()?;
    let parameter_num = cursor.read_u32()? as usize;
    let num_locals = cursor.read_u32()? as usize;
    let is_va = cursor.read_u8()? != 0;

    let upvalue_count = cursor.read_u32()? as usize;
    let mut upvalues = Vec::with_capacity(upvalue_count);
    for _ in 0..upvalue_count {
        let kind = cursor.read_u8()?;
        let index = cursor.read_u32()? as usize;
        let name = cursor.read_string()?;
        let name = pool.creat_istring(&name);
        match kind {
            0 => upvalues.push(UpValueDecl::Ref(index, name)),
            1 => upvalues.push(UpValueDecl::RefUpValue(index, name)),
            _ => return Err(format!("invalid upvalue kind {kind}")),
        }
    }

    let constant_count = cursor.read_u32()? as usize;
    let mut constants = Vec::with_capacity(constant_count);
    for _ in 0..constant_count {
        match cursor.read_u8()? {
            0 => {
                let bits = cursor.read_u64()?;
                constants.push(Value::Number(f64::from_bits(bits)));
            }
            1 => {
                let string = cursor.read_string()?;
                constants.push(Value::String(pool.creat_istring(&string)));
            }
            tag => return Err(format!("invalid constant tag {tag}")),
        }
    }

    let bytecode_len = cursor.read_u32()? as usize;
    let bytecodes = cursor.take(bytecode_len)?.to_vec();
    let line_count = cursor.read_u32()? as usize;
    let mut lines = Vec::with_capacity(line_count);
    for _ in 0..line_count {
        lines.push(cursor.read_u32()? as usize);
    }

    let child_count = cursor.read_u32()? as usize;
    let mut chunks = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        chunks.push(read_chunk(cursor, pool)?);
    }

    Ok(Chunk {
        bytecodes,
        lines,
        constants,
        chunks,
        file,
        upvalues,
        parameter_num,
        num_locals,
        is_va,
        native_id,
    })
}

fn write_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.len() > u32::MAX as usize {
        return Err("string too large for AOT image".to_string());
    }
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor { bytes, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "image cursor overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("unexpected end of SLTI image".to_string());
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_string(&mut self) -> Result<String, String> {
        let len = self.read_u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("invalid UTF-8 in image: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interned_string::StringPool;

    #[test]
    fn roundtrip() {
        let mut pool = StringPool::new();
        let mut child = Chunk::default();
        child.native_id = 1;
        child.parameter_num = 1;
        child.num_locals = 2;
        child.bytecodes = vec![1, 2, 3];
        child.lines = vec![10, 10, 11];
        child.constants.push(Value::Number(42.5));
        child
            .constants
            .push(Value::String(pool.creat_istring("hello")));
        let mut main = Chunk::default();
        main.native_id = 0;
        main.bytecodes = vec![4, 5];
        main.lines = vec![1, 1];
        main.chunks.push(child);
        let roots = vec![ImageRoot {
            path: String::new(),
            chunk: main,
        }];
        let bytes = serialize(&roots).unwrap();
        let mut new_pool = StringPool::new();
        let decoded = deserialize(&bytes, &mut new_pool).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].chunk.native_id, 0);
        assert_eq!(decoded[0].chunk.bytecodes, vec![4, 5]);
        assert_eq!(decoded[0].chunk.chunks.len(), 1);
        assert_eq!(decoded[0].chunk.chunks[0].native_id, 1);
        assert_eq!(
            decoded[0].chunk.chunks[0].constants[0].kind(),
            ValueKind::Number(42.5)
        );
        assert_eq!(
            decoded[0].chunk.chunks[0].constants[1].kind(),
            ValueKind::String(new_pool.creat_istring("hello"))
        );
    }
}
