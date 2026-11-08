//! Sloth value representation.
//!
//! The default representation is the original tagged-union enum.  Enabling the
//! `nan-boxing` feature switches the storage to a single 64 bit word:
//!
//! * ordinary `f64` values are stored verbatim;
//! * immediates (nil/bool) and heap pointers are stored in a quiet-NaN tagged
//!   word;
//! * values that do not fit in a pointer-sized payload (`Range`, iterators and
//!   NaN numbers) are reference counted boxes.
//!
//! Both representations expose the same [`ValueKind`] view so the VM and the
//! native modules can be written once.

use crate::{Array, Closure, Dict, Fiber, IString, Instance, Klass};
#[cfg(feature = "nan-boxing")]
use std::fmt;

/// A borrowed/copied view of a [`Value`].
///
/// `kind()` clones the string payload, so the returned value is always owned
/// and can outlive the borrow of the original value.  All other payloads are
/// `Copy`.
#[derive(Debug, PartialEq, Clone)]
pub enum ValueKind {
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

// ===========================================================================
// Tagged-union representation (default)
// ===========================================================================
#[cfg(not(feature = "nan-boxing"))]
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

#[cfg(not(feature = "nan-boxing"))]
impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Nil => ValueKind::Nil,
            Value::Bool(v) => ValueKind::Bool(*v),
            Value::Number(v) => ValueKind::Number(*v),
            Value::Range(l, r) => ValueKind::Range(*l, *r),
            Value::String(s) => ValueKind::String(s.clone()),
            Value::Array(p) => ValueKind::Array(*p),
            Value::Dictionary(p) => ValueKind::Dictionary(*p),
            Value::Error(p) => ValueKind::Error(*p),
            Value::Module(p) => ValueKind::Module(*p),
            Value::Closure(p) => ValueKind::Closure(*p),
            Value::NativeFunction(p) => ValueKind::NativeFunction(*p),
            Value::OpaqueData(p) => ValueKind::OpaqueData(*p),
            Value::Fiber(p) => ValueKind::Fiber(*p),
            Value::Klass(p) => ValueKind::Klass(*p),
            Value::Instance(p) => ValueKind::Instance(*p),
            Value::StringIter(s, i) => ValueKind::StringIter(s.clone(), *i),
            Value::ArrayIter(p, i) => ValueKind::ArrayIter(*p, *i),
        }
    }
}

#[cfg(not(feature = "nan-boxing"))]
impl Value {
    // The AOT backend requires NaN-boxing.  These constants only exist so the
    // backend module can be compiled in tagged-union builds; `build_executable`
    // rejects those builds at runtime.
    pub const NIL_BITS: u64 = 0;
    pub const TRUE_BITS: u64 = 1;
    pub const FALSE_BITS: u64 = 2;
}

// ===========================================================================
// NaN-boxed representation
// ===========================================================================
#[cfg(feature = "nan-boxing")]
const PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;

#[cfg(feature = "nan-boxing")]
const TAG_NIL: u8 = 0;
#[cfg(feature = "nan-boxing")]
const TAG_FALSE: u8 = 1;
#[cfg(feature = "nan-boxing")]
const TAG_TRUE: u8 = 2;
#[cfg(feature = "nan-boxing")]
const TAG_STRING: u8 = 3;
#[cfg(feature = "nan-boxing")]
const TAG_ARRAY: u8 = 4;
#[cfg(feature = "nan-boxing")]
const TAG_DICT: u8 = 5;
#[cfg(feature = "nan-boxing")]
const TAG_ERROR: u8 = 6;
#[cfg(feature = "nan-boxing")]
const TAG_MODULE: u8 = 7;
#[cfg(feature = "nan-boxing")]
const TAG_CLOSURE: u8 = 8;
#[cfg(feature = "nan-boxing")]
const TAG_NATIVE: u8 = 9;
#[cfg(feature = "nan-boxing")]
const TAG_OPAQUE: u8 = 10;
#[cfg(feature = "nan-boxing")]
const TAG_FIBER: u8 = 11;
#[cfg(feature = "nan-boxing")]
const TAG_KLASS: u8 = 12;
#[cfg(feature = "nan-boxing")]
const TAG_INSTANCE: u8 = 13;
#[cfg(feature = "nan-boxing")]
const TAG_BOXED: u8 = 14;

/// Build a tagged quiet-NaN word.
///
/// The top 16 bits are `0x7ff8 | low_tag` for tags 0..=7 and
/// `0xfff8 | low_tag` for tags 8..=15.  A values is tagged if its top 16 bits
/// match either pattern.  NaNs are boxed by `Value::Number`, so no ordinary
/// number can collide with a tag.
#[cfg(feature = "nan-boxing")]
#[inline]
const fn pack(tag: u8, payload: u64) -> u64 {
    let low = (tag & 0x7) as u16;
    let sign = ((tag >> 3) & 1) as u16;
    let top = 0x7ff8u16 | (sign << 15) | low;
    ((top as u64) << 48) | (payload & PAYLOAD_MASK)
}

#[cfg(feature = "nan-boxing")]
#[inline]
const fn is_tagged(bits: u64) -> bool {
    let top = (bits >> 48) as u16;
    (top & 0xfff8) == 0x7ff8 || (top & 0xfff8) == 0xfff8
}

#[cfg(feature = "nan-boxing")]
#[inline]
const fn tag_of(bits: u64) -> u8 {
    let top = (bits >> 48) as u16;
    let low = (top & 0x7) as u8;
    let sign = ((top >> 15) & 1) as u8;
    low | (sign << 3)
}

#[cfg(feature = "nan-boxing")]
#[inline]
const fn payload_of(bits: u64) -> u64 {
    bits & PAYLOAD_MASK
}

#[cfg(feature = "nan-boxing")]
#[inline]
fn pointer_payload<T>(ptr: *mut T) -> u64 {
    let raw = ptr as u64;
    debug_assert_eq!(
        raw & !PAYLOAD_MASK,
        0,
        "pointer does not fit into a NaN-box payload: {ptr:?}"
    );
    raw & PAYLOAD_MASK
}

#[cfg(feature = "nan-boxing")]
#[inline]
fn payload_pointer<T>(payload: u64) -> *mut T {
    payload as usize as *mut T
}

#[cfg(feature = "nan-boxing")]
enum BoxedData {
    Range(f64, f64),
    StringIter(IString, usize),
    ArrayIter(*mut Array, usize),
    Nan(f64),
}

#[cfg(feature = "nan-boxing")]
struct BoxedValue {
    refs: usize,
    data: BoxedData,
}

#[cfg(feature = "nan-boxing")]
impl BoxedValue {
    fn new(data: BoxedData) -> *mut BoxedValue {
        Box::into_raw(Box::new(BoxedValue { refs: 1, data }))
    }
}

/// NaN-boxed value: exactly one machine word.
#[cfg(feature = "nan-boxing")]
#[repr(transparent)]
pub struct Value(u64);

#[cfg(feature = "nan-boxing")]
impl Value {
    #[allow(non_snake_case, non_upper_case_globals)]
    pub const Nil: Value = Value(pack(TAG_NIL, 0));

    /// Raw bits of the nil immediate.
    pub const NIL_BITS: u64 = pack(TAG_NIL, 0);
    /// Raw bits of the true immediate.
    pub const TRUE_BITS: u64 = pack(TAG_TRUE, 0);
    /// Raw bits of the false immediate.
    pub const FALSE_BITS: u64 = pack(TAG_FALSE, 0);

    #[allow(non_snake_case)]
    pub fn Bool(value: bool) -> Value {
        Value(if value {
            pack(TAG_TRUE, 0)
        } else {
            pack(TAG_FALSE, 0)
        })
    }

    #[allow(non_snake_case)]
    pub fn Number(value: f64) -> Value {
        if value.is_nan() {
            // A raw NaN would collide with the tagging scheme.
            Value::from_boxed(BoxedData::Nan(value))
        } else {
            Value(value.to_bits())
        }
    }

    #[allow(non_snake_case)]
    pub fn Range(start: f64, end: f64) -> Value {
        Value::from_boxed(BoxedData::Range(start, end))
    }

    #[allow(non_snake_case)]
    pub fn String(value: IString) -> Value {
        let raw = pointer_payload(value.as_raw());
        // The value owns the reference that was passed in.
        std::mem::forget(value);
        Value(pack(TAG_STRING, raw))
    }

    #[allow(non_snake_case)]
    pub fn Array(ptr: *mut Array) -> Value {
        Value(pack(TAG_ARRAY, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Dictionary(ptr: *mut Dict) -> Value {
        Value(pack(TAG_DICT, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Error(ptr: *mut Dict) -> Value {
        Value(pack(TAG_ERROR, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Module(ptr: *mut Dict) -> Value {
        Value(pack(TAG_MODULE, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Closure(ptr: *mut Closure) -> Value {
        Value(pack(TAG_CLOSURE, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn NativeFunction(ptr: *mut u8) -> Value {
        Value(pack(TAG_NATIVE, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn OpaqueData(ptr: *mut u8) -> Value {
        Value(pack(TAG_OPAQUE, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Fiber(ptr: *mut Fiber) -> Value {
        Value(pack(TAG_FIBER, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Klass(ptr: *mut Klass) -> Value {
        Value(pack(TAG_KLASS, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn Instance(ptr: *mut Instance) -> Value {
        Value(pack(TAG_INSTANCE, pointer_payload(ptr)))
    }

    #[allow(non_snake_case)]
    pub fn StringIter(value: IString, index: usize) -> Value {
        Value::from_boxed(BoxedData::StringIter(value, index))
    }

    #[allow(non_snake_case)]
    pub fn ArrayIter(ptr: *mut Array, index: usize) -> Value {
        Value::from_boxed(BoxedData::ArrayIter(ptr, index))
    }

    fn from_boxed(data: BoxedData) -> Value {
        Value(pack(TAG_BOXED, pointer_payload(BoxedValue::new(data))))
    }

    pub fn kind(&self) -> ValueKind {
        let bits = self.0;
        if !is_tagged(bits) {
            return ValueKind::Number(f64::from_bits(bits));
        }
        let payload = payload_of(bits);
        match tag_of(bits) {
            TAG_NIL => ValueKind::Nil,
            TAG_FALSE => ValueKind::Bool(false),
            TAG_TRUE => ValueKind::Bool(true),
            TAG_STRING => unsafe {
                let raw = payload_pointer::<u8>(payload);
                IString::retain_raw(raw);
                ValueKind::String(IString::from_raw(raw))
            },
            TAG_ARRAY => ValueKind::Array(payload_pointer(payload)),
            TAG_DICT => ValueKind::Dictionary(payload_pointer(payload)),
            TAG_ERROR => ValueKind::Error(payload_pointer(payload)),
            TAG_MODULE => ValueKind::Module(payload_pointer(payload)),
            TAG_CLOSURE => ValueKind::Closure(payload_pointer(payload)),
            TAG_NATIVE => ValueKind::NativeFunction(payload_pointer(payload)),
            TAG_OPAQUE => ValueKind::OpaqueData(payload_pointer(payload)),
            TAG_FIBER => ValueKind::Fiber(payload_pointer(payload)),
            TAG_KLASS => ValueKind::Klass(payload_pointer(payload)),
            TAG_INSTANCE => ValueKind::Instance(payload_pointer(payload)),
            TAG_BOXED => unsafe {
                let boxed = &*payload_pointer::<BoxedValue>(payload);
                match &boxed.data {
                    BoxedData::Range(start, end) => ValueKind::Range(*start, *end),
                    BoxedData::StringIter(value, index) => {
                        ValueKind::StringIter(value.clone(), *index)
                    }
                    BoxedData::ArrayIter(ptr, index) => ValueKind::ArrayIter(*ptr, *index),
                    BoxedData::Nan(value) => ValueKind::Number(*value),
                }
            },
            _ => unreachable!("reserved NaN-box tag"),
        }
    }

    /// Raw bits of this value, without transferring ownership.
    #[inline]
    pub fn to_bits(&self) -> u64 {
        self.0
    }

    /// Consume the value and return its bits, transferring ownership of any
    /// reference counted payload to the caller.
    #[inline]
    pub fn into_bits(self) -> u64 {
        let bits = self.0;
        std::mem::forget(self);
        bits
    }

    /// Reconstruct a value from bits.  The caller takes ownership of the
    /// reference counted payload (if any) represented by `bits`.
    ///
    /// # Safety
    /// `bits` must be a valid value word; for reference counted payloads the
    /// caller must own one reference.
    #[inline]
    pub unsafe fn from_bits(bits: u64) -> Value {
        Value(bits)
    }

    /// Increment the reference count of a reference counted payload.
    #[inline]
    pub unsafe fn retain_bits(bits: u64) {
        if !is_tagged(bits) {
            return;
        }
        match tag_of(bits) {
            TAG_STRING => IString::retain_raw(payload_pointer::<u8>(payload_of(bits))),
            TAG_BOXED => {
                (*payload_pointer::<BoxedValue>(payload_of(bits))).refs += 1;
            }
            _ => {}
        }
    }

    /// Release one reference of a reference counted payload.
    #[inline]
    pub unsafe fn release_bits(bits: u64) {
        if !is_tagged(bits) {
            return;
        }
        match tag_of(bits) {
            TAG_STRING => IString::release_raw(payload_pointer::<u8>(payload_of(bits))),
            TAG_BOXED => {
                let boxed = payload_pointer::<BoxedValue>(payload_of(bits));
                (*boxed).refs -= 1;
                if (*boxed).refs == 0 {
                    drop(Box::from_raw(boxed));
                }
            }
            _ => {}
        }
    }

    /// True when the raw word is an ordinary number (not a tagged immediate or
    /// heap pointer).
    #[inline]
    pub fn is_number_bits(bits: u64) -> bool {
        !is_tagged(bits)
    }

    /// Tag of a raw value word, or `None` for plain numbers.
    #[inline]
    pub fn tag_bits(bits: u64) -> Option<u8> {
        if is_tagged(bits) {
            Some(tag_of(bits))
        } else {
            None
        }
    }
}

#[cfg(feature = "nan-boxing")]
impl Clone for Value {
    fn clone(&self) -> Self {
        let bits = self.0;
        if is_tagged(bits) {
            match tag_of(bits) {
                TAG_STRING => unsafe {
                    IString::retain_raw(payload_pointer::<u8>(payload_of(bits)));
                },
                TAG_BOXED => unsafe {
                    (*payload_pointer::<BoxedValue>(payload_of(bits))).refs += 1;
                },
                _ => {}
            }
        }
        Value(bits)
    }
}

#[cfg(feature = "nan-boxing")]
impl Drop for Value {
    fn drop(&mut self) {
        let bits = self.0;
        if !is_tagged(bits) {
            return;
        }
        match tag_of(bits) {
            TAG_STRING => unsafe {
                IString::release_raw(payload_pointer::<u8>(payload_of(bits)));
            },
            TAG_BOXED => unsafe {
                let boxed = payload_pointer::<BoxedValue>(payload_of(bits));
                (*boxed).refs -= 1;
                if (*boxed).refs == 0 {
                    drop(Box::from_raw(boxed));
                }
            },
            _ => {}
        }
    }
}

#[cfg(feature = "nan-boxing")]
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind()
    }
}

#[cfg(feature = "nan-boxing")]
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            ValueKind::Nil => write!(f, "Nil"),
            ValueKind::Bool(v) => write!(f, "Bool({v:?})"),
            ValueKind::Number(v) => write!(f, "Number({v:?})"),
            ValueKind::Range(l, r) => write!(f, "Range({l:?}, {r:?})"),
            ValueKind::String(s) => write!(f, "String({s:?})"),
            ValueKind::Array(p) => write!(f, "Array({p:?})"),
            ValueKind::Dictionary(p) => write!(f, "Dictionary({p:?})"),
            ValueKind::Error(p) => write!(f, "Error({p:?})"),
            ValueKind::Module(p) => write!(f, "Module({p:?})"),
            ValueKind::Closure(p) => write!(f, "Closure({p:?})"),
            ValueKind::NativeFunction(p) => write!(f, "NativeFunction({p:?})"),
            ValueKind::OpaqueData(p) => write!(f, "OpaqueData({p:?})"),
            ValueKind::Fiber(p) => write!(f, "Fiber({p:?})"),
            ValueKind::Klass(p) => write!(f, "Klass({p:?})"),
            ValueKind::Instance(p) => write!(f, "Instance({p:?})"),
            ValueKind::StringIter(s, i) => write!(f, "StringIter({s:?}, {i:?})"),
            ValueKind::ArrayIter(p, i) => write!(f, "ArrayIter({p:?}, {i:?})"),
        }
    }
}

// ===========================================================================
// Representation independent helpers
// ===========================================================================
impl Value {
    pub fn to_bool_v(&self) -> Value {
        Value::Bool(self.to_bool())
    }
    pub fn to_bool(&self) -> bool {
        match self.kind() {
            ValueKind::Nil => false,
            ValueKind::Bool(v) => v,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interned_string::StringPool;

    #[test]
    fn immediates_roundtrip() {
        assert_eq!(Value::Nil.kind(), ValueKind::Nil);
        assert_eq!(Value::Bool(true).kind(), ValueKind::Bool(true));
        assert_eq!(Value::Bool(false).kind(), ValueKind::Bool(false));
        assert_eq!(Value::Number(1.5).kind(), ValueKind::Number(1.5));
        assert_eq!(Value::Number(f64::INFINITY).kind(), ValueKind::Number(f64::INFINITY));
    }

    #[test]
    fn range_roundtrips_and_clones() {
        let value = Value::Range(1.0, 10.0);
        let cloned = value.clone();
        assert_eq!(cloned.kind(), ValueKind::Range(1.0, 10.0));
        drop(value);
        assert_eq!(cloned.kind(), ValueKind::Range(1.0, 10.0));
    }

    #[test]
    fn pointers_roundtrip() {
        let array = 0x1234usize as *mut Array;
        let dict = 0x5678usize as *mut Dict;
        assert_eq!(Value::Array(array).kind(), ValueKind::Array(array));
        assert_eq!(Value::Dictionary(dict).kind(), ValueKind::Dictionary(dict));
        assert_eq!(Value::Error(dict).kind(), ValueKind::Error(dict));
        assert_eq!(Value::Module(dict).kind(), ValueKind::Module(dict));
        assert_eq!(
            Value::OpaqueData(0x9abcusize as *mut u8).kind(),
            ValueKind::OpaqueData(0x9abcusize as *mut u8)
        );
    }

    #[test]
    fn strings_roundtrip() {
        let mut pool = StringPool::new();
        let string = pool.creat_istring("sloth");
        let value = Value::String(string.clone());
        assert_eq!(value.kind(), ValueKind::String(string.clone()));
        let cloned = value.clone();
        drop(value);
        assert_eq!(cloned.kind(), ValueKind::String(string));
    }

    #[cfg(feature = "nan-boxing")]
    #[test]
    fn value_is_one_word() {
        assert_eq!(std::mem::size_of::<Value>(), 8);
    }

    #[cfg(not(feature = "nan-boxing"))]
    #[test]
    fn value_is_still_a_tagged_union() {
        assert_eq!(std::mem::size_of::<Value>(), std::mem::size_of::<ValueKind>());
    }
}
