//use std::collections::HashMap;
use std::fmt::{self, Display};

#[derive(Debug)]
pub struct IString {
    id: usize,
    master: *const StringPool,
}
impl Clone for IString {
    fn clone(&self) -> Self {
        IString {
            id: self.id,
            master: self.master,
        }
    }
}
impl PartialEq for IString {
    fn eq(&self, other: &IString) -> bool {
        self.id == other.id
    }
}
impl Display for IString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let content = unsafe { &(*self.master).data[self.id] };
        write!(f, "{}", content)
    }
}
pub struct StringPool {
    data: Vec<String>,
}

impl StringPool {
    pub fn new() -> StringPool {
        StringPool { data: Vec::new() }
    }
    pub fn creat_istring(&mut self, s: &str) -> IString {
        for (index, ss) in self.data.iter().enumerate() {
            if ss == s {
                return IString {
                    id: index,
                    master: self as *const StringPool,
                };
            }
        }
        self.data.push(s.to_owned());
        IString {
            id: self.data.len() - 1,
            master: self as *const StringPool,
        }
    }
}
