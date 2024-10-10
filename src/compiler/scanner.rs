use super::*;
use std::collections::HashMap;
pub struct ScannerCtx {
    src: Vec<char>,
    pub tokens: Vec<Token>,
    pub cood: Vec<(usize, usize)>, // row & col
    ptr: usize,
    row: usize,
    col: usize,
    len: usize,
}

impl ScannerCtx {
    pub fn new(src: &str) -> ScannerCtx {
        let mut char_serial = Vec::new();
        for c in src.chars() {
            char_serial.push(c);
        }
        let t = char_serial.len();
        ScannerCtx {
            src: char_serial,
            tokens: Vec::new(),
            cood: Vec::new(),
            ptr: 0,
            row: 1,
            col: 1,
            len: t,
        }
    }
    pub fn parse(&mut self) -> Result<(), String> {
        let keyword_map: HashMap<&str, Token> = HashMap::from([
            ("and", Token::And),
            ("or", Token::Or),
            ("not", Token::Not),
            ("true", Token::True),
            ("false", Token::False),
            ("Dict", Token::Dict),
            ("while", Token::While),
            ("var", Token::Var),
            ("if", Token::If),
            ("else", Token::Else),
            ("Array", Token::Array),
            ("func", Token::Function),
            ("assign", Token::Assign),
            ("Nil", Token::Nil),
            ("return", Token::Return),
            ("except", Token::Except),
        ]);
        let single_punct_map: HashMap<char, Token> = HashMap::from([
            ('|', Token::Stick),
            ('+', Token::Add),
            ('-', Token::Sub),
            ('/', Token::LSlash),
            ('*', Token::Star),
            ('%', Token::Percent),
            ('(', Token::LParen),
            (')', Token::RParen),
            ('[', Token::LBracket),
            (']', Token::RBracket),
            ('{', Token::LBrace),
            ('}', Token::RBrace),
            (',', Token::Comma),
            ('<', Token::LArrow),
            ('>', Token::RArrow),
            (';', Token::Semicolon),
            (':', Token::Colon),
            ('=', Token::Equal),
            ('.', Token::Dot),
            ('?', Token::Question),
        ]);
        loop {
            let nxt_c = if let Some(nxt_c) = self.peek() {
                nxt_c
            } else {
                return Ok(());
            };
            match nxt_c {
                c if { c == '/' && self.peekn(2) == Some('/') } => {
                    while let Some(comment_c) = self.peek() {
                        if comment_c != '\n' {
                            self.advance()?;
                        } else {
                            break;
                        }
                    }
                }
                c if { c.is_ascii_digit() } => {
                    let x = Token::Number(self.number()?);
                    self.tokens.push(x);
                    self.record_pos();
                }
                c if { c.is_alphabetic() || c == '_' } => {
                    let s = self.identifier()?;
                    match s.as_ref() {
                        s if { keyword_map.contains_key::<str>(s) } => {
                            self.tokens.push(keyword_map[s].clone());
                        }
                        _ => {
                            let s = Token::Symbol(s);
                            self.tokens.push(s);
                        }
                    }
                    self.record_pos();
                }
                '\"' => {
                    let s = Token::String(self.string()?);
                    self.tokens.push(s);
                    self.record_pos();
                }
                c if { c.is_ascii_punctuation() } => {
                    if let Some(ahead) = self.peekn(2) {
                        if c == '!' && ahead == '=' {
                            self.tokens.push(Token::NotEqual);
                            self.record_pos();
                            self.advance()?;
                            self.advance()?;
                            continue;
                        }
                        if c == '<' && ahead == '=' {
                            self.tokens.push(Token::Le);
                            self.record_pos();
                            self.advance()?;
                            self.advance()?;
                            continue;
                        }
                        if c == '>' && ahead == '=' {
                            self.tokens.push(Token::Ge);
                            self.record_pos();
                            self.advance()?;
                            self.advance()?;
                            continue;
                        }
                        if c == '=' && ahead == '=' {
                            self.tokens.push(Token::EEqual);
                            self.record_pos();
                            self.advance()?;
                            self.advance()?;
                            continue;
                        }
                    }
                    if let Some(t) = single_punct_map.get(&c) {
                        self.tokens.push(t.clone());
                        self.record_pos();
                        self.advance()?;
                    } else {
                        return Err(self.scanner_err_str(
                            "this punctuation is not used but reserved for future",
                        ));
                    }
                }
                c if { c.is_whitespace() } => {
                    self.advance()?;
                }
                _ => {
                    return Err(self.scanner_err_str(
                    "although utf-8 is supported, I don't think it's fun to mix emoji or something else in code."));
                }
            }
        }
    }
    fn consume(&mut self, c: char) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.advance()?;
            Ok(())
        } else {
            Err(format!(
                "unexpected character in ({},{})",
                self.row, self.col
            ))
        }
    }
    #[inline]
    fn peek(&self) -> Option<char> {
        if self.ptr < self.len {
            Some(self.src[self.ptr])
        } else {
            None
        }
    }
    #[inline]
    fn peekn(&self, n: usize) -> Option<char> {
        if self.ptr + n - 1 < self.len {
            Some(self.src[self.ptr + n - 1])
        } else {
            None
        }
    }
    #[inline]
    fn advance(&mut self) -> Result<(), String> {
        if self.peek() == Some('\n') {
            self.col = 1;
            self.row += 1;
        }
        self.ptr += 1;
        self.col += 1;
        Ok(())
    }
    fn record_pos(&mut self) {
        self.cood.push((self.row, self.col));
    }
    fn identifier(&mut self) -> Result<String, String> {
        let mut ret = String::new();
        ret.push(self.peek().unwrap());
        self.advance()?;
        loop {
            let c = self.peek();
            match c {
                Some(c) => {
                    if c.is_alphanumeric() || c == '_' {
                        ret.push(c);
                        self.advance()?;
                    } else {
                        break;
                    }
                }
                None => {
                    break;
                }
            }
        }
        Ok(ret)
    }
    fn number(&mut self) -> Result<f64, String> {
        let mut ret = String::new();
        loop {
            let c = self.peek();
            match c {
                Some(c) => {
                    if c.is_ascii_digit() || c == '.' {
                        ret.push(c);
                        self.advance()?;
                    } else {
                        break;
                    }
                }
                None => {
                    break;
                }
            }
        }
        let num = ret.parse::<f64>();
        match num {
            Ok(x) => Ok(x),
            Err(_) => Err(self.scanner_err_str("illegal number format")),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let mut ret = String::new();
        self.advance()?; // consume "
        loop {
            let c = self.peek();
            match c {
                Some(c) => {
                    if c == '\"' {
                        self.advance()?;
                        break;
                    } else if c == '\\' {
                        // escaped character
                        if let Some(ahead) = self.peekn(2) {
                            let escaped_ch = match ahead {
                                '\'' => '\'',
                                '\"' => '\"',
                                'n' => '\n',
                                't' => '\t',
                                'r' => '\r',
                                '\\' => '\\',
                                _ => {
                                    return Err(
                                        self.scanner_err_str("unsupported escaped character")
                                    )
                                }
                            };
                            ret.push(escaped_ch);
                            self.advance()?;
                            self.advance()?;
                        }
                    } else {
                        ret.push(c);
                        self.advance()?;
                    }
                }
                None => {
                    return Err(self.scanner_err_str("unexpectd eof before the end of a string"));
                }
            }
        }
        Ok(ret)
    }
    #[inline]
    fn scanner_err_str(&mut self, msg: &str) -> String {
        format!("{} in ({},{})", msg, self.row, self.col)
    }
}
#[cfg(test)]
mod test {
    #[test]
    fn lex_test0() {
        use super::ScannerCtx;
        let mut scanner = ScannerCtx::new(
            r#"
            var a = 3;
            var b = 4;
            {
                var c = 5;
                var a = 6;
                var a = if (a >= c) {a} else {c};
                var f = |x|{x + 1};
            }
            "23333\n322"
        "#,
        );
        println!("{:?}", scanner.parse());
        println!("{:?}", scanner.tokens);
        println!("{:?}", scanner.cood)
    }
}
