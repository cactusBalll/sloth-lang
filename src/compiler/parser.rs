use crate::compiler::Token;
use crate::*;
use std::collections::HashMap;
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum PrattPrecedence {
    Lowest,
    Or,
    And,
    Equal,
    Cmp,
    Term,
    Factor,
    Unary,
    Call,
    Primary,
    None,
}
fn get_precedence(token: &Token) -> PrattPrecedence {
    match token {
        Token::Number(_)
        | Token::String(_)
        | Token::Symbol(_)
        | Token::True
        | Token::False
        | Token::Nil
        | Token::Dict
        | Token::Stick
        | Token::Array => PrattPrecedence::Primary,
        Token::Dot | Token::LParen | Token::LBracket => PrattPrecedence::Call,
        Token::Not => PrattPrecedence::Unary,
        Token::Sub | Token::Add => PrattPrecedence::Term,
        Token::And => PrattPrecedence::And,
        Token::Or => PrattPrecedence::Or,
        Token::LSlash | Token::Percent | Token::Star => PrattPrecedence::Factor,
        Token::EEqual | Token::NotEqual => PrattPrecedence::Equal,
        Token::Le | Token::Ge | Token::LArrow | Token::RArrow => PrattPrecedence::Cmp,
        _ => PrattPrecedence::None,
    }
}
#[derive(PartialEq)]
enum VarLoc {
    Local(usize),
    UpValue(usize),
    UpLocal(usize),
    Global(usize),
    NotFound,
}
pub struct ParserCtx {
    ptr: usize,
    len: usize,
    token_cood: Vec<(usize, usize)>,
    tokens: Vec<Token>,
    pub chunk: Vec<Chunk>,

    symbol_table: Vec<HashMap<String, usize>>,
    symbol_captured: Vec<Vec<bool>>,

    global_symbol: HashMap<String, usize>,
    depth: usize,
}

impl ParserCtx {
    pub fn new(
        tokens: Vec<Token>,
        token_cood: Vec<(usize, usize)>,
        global_symbol: HashMap<String, usize>,
    ) -> ParserCtx {
        ParserCtx {
            ptr: 0,
            len: tokens.len(),
            chunk: vec![Chunk::default()],
            tokens,
            token_cood,
            symbol_table: vec![HashMap::new()],
            symbol_captured: vec![vec![]],
            global_symbol,
            //num_upvalues: vec![0],
            depth: 0,
        }
    }
    pub fn parse_prog(&mut self) -> Result<(), String> {
        if let Some(tk) = self.tokens.last() {
            if tk != &Token::Semicolon && tk != &Token::RBrace {
                return Err("incomplete program".to_owned());
            }
        }
        if let Err(s) = self.parse() {
            if s == "EOF" {
                self.emit_with_line(Instr::Return, self.token_cood.last().unwrap().0);
                Ok(())
            } else {
                Err(s)
            }
        } else {
            Ok(())
        }
    }
    fn parse(&mut self) -> Result<(), String> {
        loop {
            let tok = if let Some(tok) = self.peek() {
                tok
            } else {
                return Ok(());
            };
            match tok {
                Token::Var => {
                    self.parse_decl()?;
                }
                Token::LBrace => {
                    self.parse_block()?;
                }
                Token::If => {
                    self.parse_if()?;
                }
                Token::While => {
                    self.parse_while()?;
                }
                Token::Function => {
                    self.parse_func_decl()?;
                }
                Token::Return => {
                    self.parse_return()?;
                }
                Token::Except => {
                    self.parse_except()?;
                }
                Token::Symbol(_)
                | Token::Array
                | Token::Dict
                | Token::Number(_)
                | Token::String(_)
                | Token::True
                | Token::False
                | Token::Not
                | Token::Nil
                | Token::Sub => {
                    self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
                    self.emit(Instr::Pop);
                    self.consume(Token::Semicolon)?;
                }
                Token::RBrace => return Ok(()),
                tk => {
                    return Err(self.parser_err_str(format!("unexpected token {tk:?}").as_ref()));
                }
            }
        }
    }
    fn parse_return(&mut self) -> Result<(), String> {
        self.consume(Token::Return)?;
        let tok = if let Some(tok) = self.peek() {
            tok
        } else {
            return Ok(());
        };
        if Token::Semicolon == tok {
            self.emit(Instr::Return);
            self.advance();
            Ok(())
        } else {
            let line = self.get_line();
            self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
            self.emit_with_line(Instr::Return, line);
            self.consume(Token::Semicolon)?;
            Ok(())
        }
    }
    fn parse_except(&mut self) -> Result<(), String> {
        self.consume(Token::Except)?;

        let line = self.get_line();
        self.parse_rval_expr(PrattPrecedence::Lowest, None)?;

        self.emit_with_line(Instr::Except, line);
        self.consume(Token::Semicolon)?;
        Ok(())
    }
    #[inline]
    fn parse_argument(&mut self) -> Result<usize, String> {
        let mut argument_num = 0;
        let tok = self.peek_not_eof()?;
        if Token::RParen == tok {
            return Ok(0);
        }
        loop {
            self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
            argument_num += 1;
            let tk = self.peek_not_eof()?;
            if Token::Comma == tk {
                self.advance();
            } else if Token::RParen == tk {
                return Ok(argument_num);
            } else {
                return Err(self.parser_err_str("illegal argument list"));
            }
        }
    }
    #[inline]
    fn emit_get_symbol(&mut self, symbol: &str, line: usize) -> Result<(), String> {
        match self.resolve_local(symbol) {
            VarLoc::Local(x) => {
                self.emit_with_line(Instr::GetLocal(x), line);
            }
            VarLoc::UpLocal(x) => {
                self.emit_with_line(Instr::GetUpValue(x), line);
            }
            VarLoc::UpValue(x) => {
                self.emit_with_line(Instr::GetUpValue(x), line);
            }
            VarLoc::Global(x) => {
                self.emit_with_line(Instr::GetGlobal(x), line);
            }
            VarLoc::NotFound => {
                return Err(self.parser_err_str((format!("symbol not found:{}", symbol)).as_str()));
            }
        }
        Ok(())
    }
    #[inline]
    fn emit_set_symbol(&mut self, symbol: &str, line: usize) -> Result<(), String> {
        match self.resolve_local(symbol) {
            VarLoc::Local(x) => {
                self.emit_with_line(Instr::SetLocal(x), line);
            }
            VarLoc::UpLocal(x) => {
                self.emit_with_line(Instr::SetUpValue(x), line);
            }
            VarLoc::UpValue(x) => {
                self.emit_with_line(Instr::SetUpValue(x), line);
            }
            _ => {
                return Err(self.parser_err_str((format!("symbol not found:{}", symbol)).as_str()));
            }
        }
        Ok(())
    }
    fn parse_func_decl(&mut self) -> Result<(), String> {
        self.consume(Token::Function)?;
        let symbol;
        if let Token::Symbol(s) = self.peek_not_eof()? {
            symbol = s;
        } else {
            return Err(self.parser_err_str("invalid function declaration."));
        }
        self.advance();
        self.add_local(symbol)?;
        self.open_env();
        self.consume(Token::LParen)?;
        let mut para_num = 0;
        while let Token::Symbol(s) = self.peek_not_eof()? {
            self.advance();
            self.add_local(s)?;
            para_num += 1;
            match self.consume(Token::Comma) {
                Ok(()) => {}
                Err(_) => {
                    if Token::RParen == self.peek_not_eof()? {
                        //self.advance();
                        break;
                    } else {
                        return Err(self.parser_err_str("expect RParen after parameter list"));
                    }
                }
            }
        }
        self.consume(Token::RParen)?;
        self.consume(Token::LBrace)?;
        self.parse()?;
        self.consume(Token::RBrace)?;
        let mut chunk = self.close_env();
        chunk.parameter_num = para_num;
        self.load_value(Value::Chunk(chunk));
        self.emit(Instr::SetLocal(self.chunk[self.depth].num_locals - 1));
        self.emit(Instr::Pop);
        Ok(())
    }
    fn parse_decl(&mut self) -> Result<(), String> {
        self.consume(Token::Var)?;
        let symbol;
        if let Token::Symbol(s) = self.peek_not_eof()? {
            symbol = s;
        } else {
            return Err(self.parser_err_str("invalid declaration statement"));
        }
        self.advance();
        if let Some(tok) = self.peek() {
            //declaration with assignment
            if Token::Equal == tok {
                self.advance();
                self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
                self.add_local(symbol)?;
                self.emit(Instr::SetLocal(self.chunk[self.depth].num_locals - 1));
                self.emit(Instr::Pop);
                return Ok(());
            }
        }
        self.add_local(symbol)?;
        self.consume(Token::Semicolon)?;
        Ok(())
    }
    fn parse_while(&mut self) -> Result<(), String> {
        self.consume(Token::While)?;
        self.consume(Token::LParen)?;
        let jumpback_point = self.chunk[self.depth].bytecodes.len();
        self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
        let patch_point = self.chunk[self.depth].bytecodes.len();
        self.emit(Instr::JumpIfNot(0)); // to be patched
        self.consume(Token::RParen)?;
        self.consume(Token::LBrace)?;
        self.parse()?;
        self.emit(Instr::Jump(
            jumpback_point as i32 - self.chunk[self.depth].bytecodes.len() as i32,
        ));
        self.consume(Token::RBrace)?;
        self.chunk[self.depth].bytecodes[patch_point] =
            Instr::JumpIfNot(self.chunk[self.depth].bytecodes.len() as i32 - patch_point as i32);
        Ok(())
    }
    fn parse_block(&mut self) -> Result<(), String> {
        self.consume(Token::LBrace)?;
        self.open_env();
        self.parse()?;

        let res = self.close_env();
        self.emit(Instr::Load(self.chunk[self.depth].constants.len()));
        self.emit(Instr::Call(0));
        self.add_annoymos_closure(res);
        self.consume(Token::RBrace)?;
        Ok(())
    }
    fn parse_if(&mut self) -> Result<(), String> {
        self.consume(Token::If)?;
        self.consume(Token::LParen)?;
        self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
        let patch_point = self.chunk[self.depth].bytecodes.len();
        // emit an empty slot, jump to FALSE branch but FALSE branch is now not parsed.
        self.emit(Instr::JumpIfNot(0));
        self.consume(Token::RParen)?;
        self.consume(Token::LBrace)?;
        self.parse()?;
        let patch_point2 = self.chunk[self.depth].bytecodes.len();
        // emit an empty slot, jump to end of if statement, but we still don't know if there
        // is an else clause, Jump(0) is just nop.
        self.emit(Instr::Jump(0));
        self.consume(Token::RBrace)?;
        self.chunk[self.depth].bytecodes[patch_point] =
            Instr::JumpIfNot((self.chunk[self.depth].bytecodes.len() - patch_point) as i32);
        if let Some(tok) = self.peek() {
            if Token::Else == tok {
                self.advance();
                self.consume(Token::LBrace)?;
                self.parse()?;
                self.consume(Token::RBrace)?;
                self.chunk[self.depth].bytecodes[patch_point2] =
                    Instr::Jump((self.chunk[self.depth].bytecodes.len() - patch_point2) as i32);
            }
        }
        Ok(())
    }
    #[inline]
    fn open_env(&mut self) {
        self.chunk.push(Chunk::default());
        self.symbol_table.push(HashMap::new());
        self.symbol_captured.push(Vec::new());
        self.depth += 1;
    }
    #[inline]
    fn close_env(&mut self) -> Chunk {
        self.emit_with_line(Instr::Return, 0);
        self.depth -= 1;
        self.symbol_table.pop();
        self.symbol_captured.pop();
        self.chunk.pop().unwrap()
    }
    #[inline]
    fn add_annoymos_closure(&mut self, chunk: Chunk) {
        self.chunk[self.depth].constants.push(Value::Chunk(chunk));
    }
    fn add_local(&mut self, symbol: String) -> Result<(), String> {
        if self.symbol_table[self.depth].get(&symbol) != None {
            return Err(self.parser_err_str("redeclaration of symbol"));
        }
        self.symbol_table[self.depth].insert(symbol, self.chunk[self.depth].num_locals);
        self.symbol_captured[self.depth].push(false);
        self.chunk[self.depth].num_locals += 1;
        Ok(())
    }
    pub fn parse_rval_expr(
        &mut self,
        prec: PrattPrecedence,
        decl_symbol: Option<&str>,
    ) -> Result<(), String> {
        // Pratt Parser
        match self.peek_not_eof()? {
            Token::Symbol(s) => {
                match self.resolve_local(&s) {
                    VarLoc::Local(x) => self.emit(Instr::GetLocal(x)),
                    VarLoc::UpValue(x) => self.emit(Instr::GetUpValue(x)),
                    VarLoc::UpLocal(x) => self.emit(Instr::GetUpValue(x)),
                    VarLoc::Global(x) => self.emit(Instr::GetGlobal(x)),
                    _ => {
                        return Err(self.parser_err_str("symbol not found"));
                    }
                }
                self.advance();
            }
            Token::Number(x) => {
                self.load_value(Value::Number(x));
                self.advance();
            }
            Token::String(s) => {
                self.load_value(Value::String(s));
                self.advance();
            }
            Token::True => {
                self.emit(Instr::LoadTrue);
                self.advance();
            }
            Token::False => {
                self.emit(Instr::LoadFalse);
                self.advance();
            }
            Token::Nil => {
                self.emit(Instr::PushNil);
            }

            Token::Array => {
                self.parse_array()?;
            }
            Token::Dict => {
                self.parse_dict()?;
            }
            Token::Sub => {
                let line = self.token_cood[self.ptr].0;
                self.advance();
                self.parse_rval_expr(PrattPrecedence::Unary, decl_symbol)?;
                self.emit_with_line(Instr::Negate, line);
            }
            Token::Not => {
                let line = self.token_cood[self.ptr].0;
                self.advance();
                self.parse_rval_expr(PrattPrecedence::Unary, decl_symbol)?;
                self.emit_with_line(Instr::Not, line);
            }
            Token::LParen => {
                self.advance();
                self.parse_rval_expr(PrattPrecedence::Lowest, decl_symbol)?;
                self.consume(Token::RParen)?;
            }
            _ => {
                //println!("{:?}", c);
                return Ok(());
                //unimplemented!();
            }
        }
        while let Some(tk) = self.peek() {
            let line = self.get_line();
            if tk == Token::Semicolon {
                break;
            }
            if tk == Token::LParen {
                //call
                let line = self.get_line();
                self.advance();
                let arg_num = self.parse_argument()?;
                self.consume(Token::RParen)?;
                self.emit_with_line(Instr::Call(arg_num), line);

                continue;
            }
            if tk == Token::LBracket {
                let line = self.get_line();
                self.advance();
                self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
                self.consume(Token::RBracket)?;
                self.emit_with_line(Instr::GetCollection, line);
                continue;
            }
            if tk == Token::Dot {
                let line = self.get_line();
                self.advance();
                if let Token::Symbol(s) = self.peek_not_eof()? {
                    self.load_value(Value::String(s));
                    self.advance();
                } else {
                    return Err(self.parser_err_str("invalid rval expr"));
                }
                self.emit_with_line(Instr::GetCollection, line);
                continue;
            }
            let nprec = get_precedence(&tk);
            if nprec != PrattPrecedence::None {
                if get_precedence(&tk) <= prec {
                    break;
                }
                self.advance();
                self.parse_rval_expr(nprec, decl_symbol)?;
            } else {
                return Ok(());
            }
            match tk {
                Token::Add => {
                    self.emit_with_line(Instr::Add, line);
                }
                Token::Sub => {
                    self.emit_with_line(Instr::Sub, line);
                }
                Token::LSlash => {
                    self.emit_with_line(Instr::Div, line);
                }
                Token::Percent => {
                    self.emit_with_line(Instr::Mod, line);
                }
                Token::Star => {
                    self.emit_with_line(Instr::Mul, line);
                }
                Token::LArrow => {
                    self.emit_with_line(Instr::Lt, line);
                }
                Token::RArrow => {
                    self.emit_with_line(Instr::Gt, line);
                }
                Token::Le => {
                    self.emit_with_line(Instr::Le, line);
                }
                Token::Ge => {
                    self.emit_with_line(Instr::Ge, line);
                }
                Token::EEqual => {
                    self.emit_with_line(Instr::Eq, line);
                }
                Token::NotEqual => {
                    self.emit_with_line(Instr::Ne, line);
                }
                Token::And => {
                    self.emit_with_line(Instr::And, line);
                }
                Token::Or => {
                    self.emit_with_line(Instr::Or, line);
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn parse_array(&mut self) -> Result<(), String> {
        self.consume(Token::Array)?;
        let line = self.get_line();
        self.consume(Token::LParen)?;
        let arg_num = self.parse_argument()?;
        self.emit_with_line(Instr::InitArray(arg_num), line);
        self.consume(Token::RParen)?;
        Ok(())
    }
    fn parse_dict(&mut self) -> Result<(), String> {
        self.consume(Token::Dict)?;
        let line = self.get_line();
        self.consume(Token::LParen)?;
        let mut num_arg = 0;
        loop {
            let tk = self.peek_not_eof()?;
            if let Token::String(s) = tk {
                self.load_value(Value::String(s));
            } else if Token::RParen == tk {
                // empty Dict
                self.advance();
                self.emit_with_line(Instr::InitDict(num_arg), line);
            } else {
                return Err(self.parser_err_str("illegal dict initialization."));
            }
            self.advance();
            self.consume(Token::Colon)?;
            self.parse_rval_expr(PrattPrecedence::Lowest, None)?;
            if Token::RParen == self.peek_not_eof()? {
                num_arg += 1;
                break;
            } else {
                self.consume(Token::Comma)?;
                num_arg += 1;
            }
        }
        self.consume(Token::RParen)?;
        self.emit_with_line(Instr::InitDict(num_arg), line);
        Ok(())
    }

    fn prefix_symbol(&mut self, _prec: PrattPrecedence) -> Result<(), String> {
        Ok(())
    }
    fn consume(&mut self, token: Token) -> Result<(), String> {
        if let Some(look_tok) = self.peek() {
            if look_tok == token {
                return Ok(());
            } else {
                return Err(format!(
                    "unexpected token in ({},{}), expect {:?}, get {:?}",
                    self.token_cood[self.ptr].0, self.token_cood[self.ptr].1, token, look_tok,
                ));
            }
        }

        return Err(format!(
            "unexpected token in ({},{}), expect {:?}, get EOF",
            self.token_cood[self.ptr].0, self.token_cood[self.ptr].1, token,
        ));
    }
    fn parser_err_str(&self, s: &str) -> String {
        format!("{} in {:?}", s, self.token_cood[self.ptr])
    }
    #[inline]
    fn peek(&self) -> Option<Token> {
        if self.ptr >= self.len {
            None
        } else {
            Some(self.tokens[self.ptr].clone())
        }
    }

    fn peek_not_eof(&self) -> Result<Token, String> {
        if self.ptr >= self.len {
            Err(self.parser_err_str("unexpected EOF"))
        } else {
            Ok(self.tokens[self.ptr].clone())
        }
    }
    #[inline]
    fn peek2(&mut self) -> Option<Token> {
        if self.ptr + 1 >= self.len {
            None
        } else {
            Some(self.tokens[self.ptr + 1].clone())
        }
    }
    #[inline]
    fn advance(&mut self) {
        self.ptr += 1;
    }
    #[inline]
    fn emit(&mut self, instr: Instr) {
        self.chunk[self.depth].bytecodes.push(instr);
        if self.ptr >= self.len {
            self.chunk[self.depth].lines.push(self.len);
        } else {
            self.chunk[self.depth]
                .lines
                .push(self.token_cood[self.ptr].0);
        }
    }
    #[inline]
    fn emit_with_line(&mut self, instr: Instr, line: usize) {
        self.chunk[self.depth].bytecodes.push(instr);
        self.chunk[self.depth].lines.push(line);
    }
    #[inline]
    fn push_constant(&mut self, c: Value) {
        self.chunk[self.depth].constants.push(c);
    }
    #[inline]
    fn load_value(&mut self, v: Value) {
        if let Some(x) = self.chunk[self.depth]
            .constants
            .iter()
            .position(|val| val == &v)
        {
            self.emit(Instr::Load(x));
        } else {
            self.push_constant(v);
            self.emit(Instr::Load(self.chunk[self.depth].constants.len() - 1));
        }
    }
    #[inline]
    fn resolve_local(&mut self, symbol: &str) -> VarLoc {
        if let Some(x) = self.symbol_table[self.depth].get(symbol) {
            return VarLoc::Local(*x);
        } else if let Some(x) = self.chunk[self.depth]
            .upvalues
            .iter()
            .position(|upv| match upv {
                UpValueDecl::Ref(_, s) => s == symbol,
                UpValueDecl::RefUpValue(_, s) => s == symbol,
            })
        {
            return VarLoc::UpValue(x);
        } else if self.depth >= 1 {
            match self.resolve(symbol, self.depth - 1) {
                VarLoc::UpLocal(x) => {
                    self.chunk[self.depth]
                        .upvalues
                        .push(UpValueDecl::Ref(x, symbol.to_owned()));
                    return VarLoc::UpValue(self.chunk[self.depth].upvalues.len() - 1);
                }
                VarLoc::UpValue(x) => {
                    self.chunk[self.depth]
                        .upvalues
                        .push(UpValueDecl::RefUpValue(x, symbol.to_owned()));
                    return VarLoc::UpValue(self.chunk[self.depth].upvalues.len() - 1);
                }
                _ => {}
            }
        }
        if let Some(x) = self.global_symbol.get(symbol) {
            return VarLoc::Global(*x);
        }
        VarLoc::NotFound
    }
    fn resolve(&mut self, symbol: &str, dep: usize) -> VarLoc {
        if let Some(x) = self.symbol_table[dep].get(symbol) {
            self.symbol_captured[dep][*x] = true;
            return VarLoc::UpLocal(*x);
        }
        if let Some(x) = self.chunk[dep].upvalues.iter().position(|upv| match upv {
            UpValueDecl::Ref(_, s) => s == symbol,
            UpValueDecl::RefUpValue(_, s) => s == symbol,
        }) {
            return VarLoc::UpValue(x);
        }
        if dep == 0 {
            return VarLoc::NotFound;
        }
        match self.resolve(symbol, dep - 1) {
            VarLoc::UpLocal(x) => {
                self.chunk[dep]
                    .upvalues
                    .push(UpValueDecl::Ref(x, symbol.to_owned()));
                VarLoc::UpValue(self.chunk[dep].upvalues.len() - 1)
            }
            VarLoc::UpValue(x) => {
                self.chunk[dep]
                    .upvalues
                    .push(UpValueDecl::RefUpValue(x, symbol.to_owned()));
                VarLoc::UpValue(self.chunk[dep].upvalues.len() - 1)
            }
            VarLoc::NotFound => VarLoc::NotFound,
            _ => VarLoc::NotFound,
        }
    }
    #[inline]
    fn get_line(&self) -> usize {
        self.token_cood[self.ptr].0
    }
}
#[cfg(test)]
mod test {
    #[test]
    fn parse_test0() {
        let src = r#"
            (3 + 42) * 5 / 3 % 2 * (2 + 5) + 4;
        "#;
        use crate::compiler::scanner::ScannerCtx;
        use std::collections::HashMap;
        let mut scanner = ScannerCtx::new(src);
        println!("{:?}", scanner.parse());
        println!("{:?}", scanner.tokens);
        println!("{:?}", scanner.cood);
        use super::{ParserCtx, PrattPrecedence};
        let mut parser = ParserCtx::new(scanner.tokens, scanner.cood, HashMap::new());
        println!(
            "{:?}",
            parser.parse_rval_expr(PrattPrecedence::Lowest, None)
        );
        println!("{:?}", parser.chunk[0].bytecodes);
        println!("{:?}", parser.chunk[0].lines);
        println!("{:?}", parser.chunk[0].constants);
    }
    #[test]
    fn parse_test1() {
        let src = r#"
            func f(a,b){
                var c = 3;
                func g(){
                    assign c = c + 1;
                }
            }
            var a = 3;
            if (a){
                assign a = a - 1;
            } else {
                assign a = a + 1;
            }
            while(a){
                assign a = a - 1;
            }
            
        "#;
        use crate::compiler::scanner::ScannerCtx;
        use std::collections::HashMap;
        let mut scanner = ScannerCtx::new(src);
        println!("{:?}", scanner.parse());
        println!("{:?}", scanner.tokens);
        println!("{:?}", scanner.cood);
        use super::{ParserCtx, PrattPrecedence};
        let mut parser = ParserCtx::new(scanner.tokens, scanner.cood, HashMap::new());
        println!("{:?}", parser.parse());
        println!("code:{:?}", parser.chunk[0]);
    }
    #[test]
    fn parse_test2() {
        let src = r#"
            var a = 0;
            assign a = a + 1;
            var d = Dict("a">4+3,"b">Array(2,4));
            var t = Array(2,4,Array(2,3));
            assign t[2][0] = 3;
        "#;
        use crate::compiler::scanner::ScannerCtx;
        use std::collections::HashMap;
        let mut scanner = ScannerCtx::new(src);
        println!("{:?}", scanner.parse());
        println!("{:?}", scanner.tokens);
        println!("{:?}", scanner.cood);
        use super::ParserCtx;
        let mut parser = ParserCtx::new(scanner.tokens, scanner.cood, HashMap::new());
        println!("{:?}", parser.parse());
        println!("code:{:?}", parser.chunk[0]);
    }
}
