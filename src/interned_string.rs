//use std::collections::HashMap;
use std::fmt::{self, Debug, Display};
use std::hash::Hash;
use std::ptr;

#[derive(Eq, Hash)]
pub struct IString {
    data: *mut StringPoolEntry,
}
impl Clone for IString {
    fn clone(&self) -> Self {
        unsafe {
            (*self.data).ref_count += 1;
        }
        IString { data: self.data }
    }
}

impl PartialEq for IString {
    fn eq(&self, other: &IString) -> bool {
        self.data == other.data
    }
}
impl Display for IString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = unsafe { &(*self.data).data };
        write!(f, "{}", s)
    }
}

impl Debug for IString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = unsafe { &(*self.data).data };
        write!(f, "IString({:?},{})", self.data, s)
    }
}

impl Drop for IString {
    fn drop(&mut self) {
        unsafe {
            (*self.data).ref_count -= 1;
        }
    }
}

impl IString {
    pub fn get_inner(&self) -> &str {
        unsafe { &(*self.data).data }
    }
}
struct StringPoolEntry {
    data: String,
    ref_count: usize,
}
pub struct StringPool {
    data: Vec<*mut StringPoolEntry>,
}

impl StringPool {
    pub fn new() -> StringPool {
        StringPool { data: Vec::new() }
    }
    pub fn creat_istring(&mut self, s: &str) -> IString {
        for (index, ss_entry) in self.data.iter().enumerate() {
            let ss = unsafe { &(**ss_entry).data };
            if ss == s {
                unsafe {
                    (**ss_entry).ref_count += 1;
                }
                return IString { data: *ss_entry };
            }
        }
        let entry = Box::new(StringPoolEntry {
            data: s.to_owned(),
            ref_count: 1,
        });
        let entry = Box::into_raw(entry);
        self.data.push(entry);

        // clean up
        for ss_entry in self.data.iter_mut() {
            unsafe {
                if (**ss_entry).ref_count <= 0 {
                    // let Box destruct it
                    let _to_drop = Box::from_raw(*ss_entry);
                    *ss_entry = ptr::null_mut() as *mut StringPoolEntry;
                }
            }
        }
        self.data
            .retain(|e| *e != ptr::null_mut() as *mut StringPoolEntry);
        IString { data: entry }
    }
}
