pub mod parser;
pub mod scanner;

use crate::interned_string::IString;
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Number(f64),
    String(IString),
    Symbol(IString),
    True,
    False,
    Dict,
    Stick,
    Nil,

    And,
    Or,
    Not,
    Add,
    Sub,
    LSlash,
    Mod,

    Percent,
    Star,
    Dot,
    Question,

    While,
    For,
    Break,
    Continue,
    Var,
    If,
    Else,

    LParen,
    RParen,

    LBracket,
    RBracket,

    LBrace,
    RBrace,

    Comma,
    LArrow,
    RArrow,
    Semicolon,
    Colon,
    Equal,
    EEqual,

    NotEqual,
    Le,
    Ge,
    Array,
    Function,

    Return,
    Except,

    PipeOp,    // |>
    AddAssign, // +=
    SubAssign, // -=
    MulAssign, // *=
    DivAssign, // /=
    ModAssign, // %=

    Class,
    Super,    //super.
    This,
    Is,

    Dots,     //..
    DotsEq,   //..=
    ThreeDots, //...

    InterplotBegin, // special token ${
    InterplotEnd,   // special token }
}

impl Token {
    pub fn is_assign(&self) -> bool {
        *self == Token::Equal
            // || self.is_op_assign()
    }

    pub fn is_op_assign(&self) -> bool {
        *self == Token::AddAssign
            || *self == Token::SubAssign
            || *self == Token::MulAssign
            || *self == Token::DivAssign
            || *self == Token::ModAssign
    }
}
