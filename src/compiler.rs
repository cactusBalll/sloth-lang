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
    Assign,

    Return,
    Except,
}
