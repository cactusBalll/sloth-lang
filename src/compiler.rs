pub mod parser;
pub mod scanner;
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Number(f64),
    String(String),
    Symbol(String),
    True,
    False,
    Matrix,
    Vec2,
    Vec3,
    Dict,
    Stick,
    Nil,

    And,
    Or,
    Not,
    Add,
    Sub,
    LSlash,

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
