use interned_string::{IString, StringPool};

use crate::compiler::scanner::ScannerResult;
use crate::compiler::Token;
use crate::*;
use std::collections::{HashMap, HashSet};
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum PrattPrecedence {
    Lowest,
    PipeOp,
    IsOp,
    Range,
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
        Token::Dots | Token::DotsEq => PrattPrecedence::Range,
        Token::Is => PrattPrecedence::IsOp,
        Token::PipeOp => PrattPrecedence::PipeOp,
        _ => PrattPrecedence::None,
    }
}
#[derive(PartialEq)]
enum VarLoc {
    Local(usize),
    UpValue(usize),
    Global(usize),
    ThisRef,
    NotFound,
}
pub struct ParserCtx<'a> {
    ptr: usize,
    len: usize,
    token_cood: Vec<(usize, usize)>,
    tokens: Vec<Token>,
    pub chunk: Vec<Chunk>, // stack of chunks

    func_ctx_stack: Vec<FuncCtx>,

    /// actually foregin(builtin) functions)
    global_symbol: HashMap<String, usize>,
    /// every toplevel symbols are exported
    exported_symbols: Vec<IString>,
    string_pool: &'a mut StringPool,
    depth: usize,

    method_ctx: bool,
}

pub struct ParserResult {
    pub chunk: Chunk,
    pub exported_symbols: Vec<IString>,
}
#[derive(Debug, Default)]
struct FuncCtx {
    block_ctx_stack: Vec<BlockCtx>,
    num_locals: usize,
    loop_ctx: bool,
    loop_ctx_stack: Vec<LoopCtx>,
}
#[derive(Debug, Default)]
struct BlockCtx {
    symbol_table: HashMap<IString, usize>,
    /// map[local_slot, is_captured]
    symbol_captured: Vec<bool>,
}
#[derive(Debug, Default)]
struct LoopCtx {
    continue_patch_point: Vec<usize>,
    break_patch_point: Vec<usize>,
}
impl<'a> ParserCtx<'a> {
    pub fn new(
        scanner_result: ScannerResult,
        global_symbol: HashMap<String, usize>,
        string_pool: &'a mut StringPool,
    ) -> ParserCtx<'a> {
        ParserCtx {
            ptr: 0,
            len: scanner_result.tokens.len(),
            chunk: vec![Chunk::default()],
            tokens: scanner_result.tokens,
            token_cood: scanner_result.cood,
            func_ctx_stack: vec![FuncCtx {
                block_ctx_stack: vec![BlockCtx::default()],
                num_locals: 0,
                loop_ctx: false,
                loop_ctx_stack: Vec::new(),
            }],
            global_symbol,
            exported_symbols: Vec::new(),
            string_pool,
            //num_upvalues: vec![0],
            depth: 0,

            method_ctx: false,
        }
    }

    pub fn finish(mut self) -> ParserResult {
        ParserResult {
            chunk: self.chunk.pop().unwrap(),
            exported_symbols: self.exported_symbols,
        }
    }
    pub fn parse_prog(&mut self) -> Result<(), String> {
        if let Some(tk) = self.tokens.last() {
            if tk != &Token::Semicolon && tk != &Token::RBrace {
                return Err("incomplete program".to_owned());
            }
        }
        loop {
            if self.peek() == None {
                break;
            }
            self.parse()?;
        }
        self.emit(Instr::Return);
        Ok(())
    }
    fn parse(&mut self) -> Result<(), String> {
        let tok = if let Some(tok) = self.peek() {
            tok
        } else {
            return Ok(());
        };
        match tok {
            Token::Break => {
                if !self.func_ctx_stack.last().unwrap().loop_ctx {
                    return Err(self.parser_err_str("break can ONLY be used inside loops"));
                } else {
                    self.advance();
                    self.advance();
                    self.emit(Instr::Nop);
                    self.func_ctx_stack
                        .last_mut()
                        .unwrap()
                        .loop_ctx_stack
                        .last_mut()
                        .unwrap()
                        .break_patch_point
                        .push(self.chunk[self.depth].bytecodes.len());
                }
            }
            Token::Continue => {
                if !self.func_ctx_stack.last().unwrap().loop_ctx {
                    return Err(self.parser_err_str("continue can ONLY be used inside loops"));
                } else {
                    self.advance();
                    self.advance();
                    self.emit(Instr::Nop);
                    self.func_ctx_stack
                        .last_mut()
                        .unwrap()
                        .loop_ctx_stack
                        .last_mut()
                        .unwrap()
                        .continue_patch_point
                        .push(self.chunk[self.depth].bytecodes.len());
                }
            }
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
            Token::For => {
                self.parse_for()?;
            }
            Token::Function => {
                self.parse_func_decl()?;
            }
            Token::Return => {
                self.parse_return()?;
            }
            Token::Symbol(_) => {
                self.parse_assign_or_rval_expr()?;
                // self.emit(Instr::Pop);
                self.consume(Token::Semicolon)?;
            }
            Token::Class => {
                self.parse_class_decl()?;
            }

            Token::LBracket
            | Token::LParen
            | Token::Dict
            | Token::Number(_)
            | Token::String(_)
            | Token::True
            | Token::False
            | Token::Not
            | Token::Nil
            | Token::Sub
            | Token::Super
            | Token::Stick
            | Token::This => {
                self.parse_rval_expr(PrattPrecedence::Lowest)?;
                self.emit(Instr::Pop);
                self.consume(Token::Semicolon)?;
            }
            Token::RBrace => return Ok(()),
            tk => {
                return Err(self.parser_err_str(format!("unexpected token {tk:?}").as_ref()));
            }
        }
        Ok(())
    }
    fn parse_class_decl(&mut self) -> Result<(), String> {
        self.advance();
        let class_name = if let Token::Symbol(class_name) = self.peek_not_eof()? {
            self.add_local(&class_name)?;
            class_name
        } else {
            return Err(self.parser_err_str("expect class name `Symbol`"));
        };
        self.advance();
        self.emit(Instr::InitClass);
        //:SuperClass
        if let Token::Colon = self.peek_not_eof()? {
            self.advance();
            if let Token::Symbol(super_class_name) = self.peek_not_eof()? {
                self.emit_get_symbol(&super_class_name, self.get_line())?;
                self.emit(Instr::ClassExtend);
            } else {
                return Err(self.parser_err_str("expect superclass name `Symbol`"));
            }
            self.advance();
        }
        self.consume(Token::LBrace)?;
        self.method_ctx = true;
        while let Token::Function = self.peek_not_eof()? {
            self.advance();
            let method_name = if let Token::Symbol(method_name) = self.peek_not_eof()? {
                method_name
            } else {
                return Err(self.parser_err_str("expect method name `Symbol`"));
            };
            self.advance();
            self.load_value(Value::String(method_name.clone()));

            let line = self.get_line();
            self.open_env();
            self.consume(Token::LParen)?;
            let mut para_num = 0;
            while let Token::Symbol(s) = self.peek_not_eof()? {
                self.advance();
                self.add_local(&s)?;
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
            let tk = self.peek_not_eof()?;
            if tk == Token::ThreeDots {
                // varidic parameters
                self.advance();
                self.chunk[self.depth].is_va = true;
            }
            self.consume(Token::RParen)?;
            self.consume(Token::LBrace)?;
            self.parse_stmt_list()?;
            if method_name.get_inner() == "__init__" {
                // __init__() method implicitly return this
                self.emit(Instr::GetThis);
                self.emit(Instr::Return);
            }
            self.consume(Token::RBrace)?;
            let mut chunk = self.close_env();
            chunk.parameter_num = para_num;
            self.chunk.last_mut().unwrap().chunks.push(chunk);
            self.emit(Instr::LoadChunk(
                self.chunk.last().unwrap().chunks.len() - 1,
            ));
            self.emit(Instr::AddMethod);
        }
        self.method_ctx = false;
        // klass on the top of stack
        self.emit_set_symbol(&class_name, self.get_line())?;
        self.consume(Token::RBrace)?;
        Ok(())
    }
    fn parse_for(&mut self) -> Result<(), String> {
        self.advance();
        self.consume(Token::LParen)?;
        self.open_block();
        self.consume(Token::Var)?;
        if let Token::Symbol(iter_var) = self.peek_not_eof()? {
            self.add_local(&iter_var)?;
        } else {
            return Err(self.parser_err_str("expect iterate variable `Symbol`"));
        }
        self.advance();
        let iter_var_slot = self.chunk[self.depth].num_locals - 1;
        self.consume(Token::Colon)?;
        self.parse_rval_expr(PrattPrecedence::Lowest)?;
        self.consume(Token::RParen)?;
        self.emit(Instr::Iterator);
        let loop_start_point = self.chunk[self.depth].bytecodes.len();
        self.emit(Instr::Next);
        let backpatch_point = self.chunk[self.depth].bytecodes.len();
        self.emit(Instr::Nop);
        self.emit(Instr::SetLocal(iter_var_slot));
        // self.emit(Instr::Pop);

        self.consume(Token::LBrace)?;
        self.parse_stmt_list()?;
        self.consume(Token::RBrace)?;
        self.emit(Instr::Jump(
            loop_start_point as i32 - self.chunk[self.depth].bytecodes.len() as i32,
        ));
        self.chunk[self.depth].bytecodes[backpatch_point] = Instr::JumpIfNot(
            self.chunk[self.depth].bytecodes.len() as i32 - backpatch_point as i32,
        );
        // pop Nil
        self.emit(Instr::Pop);
        // pop iterator
        self.emit(Instr::Pop);
        self.close_block();
        Ok(())
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
            self.parse_rval_expr(PrattPrecedence::Lowest)?;
            self.emit_with_line(Instr::Return, line);
            self.consume(Token::Semicolon)?;
            Ok(())
        }
    }
    #[inline]
    fn parse_argument(&mut self) -> Result<usize, String> {
        let mut argument_num = 0;
        let tok = self.peek_not_eof()?;
        if Token::RParen == tok || Token::RBracket == tok {
            return Ok(0);
        }
        loop {
            self.parse_rval_expr(PrattPrecedence::Lowest)?;
            argument_num += 1;
            let tk = self.peek_not_eof()?;
            if Token::Comma == tk {
                self.advance();
            } else if Token::RParen == tk || Token::RBracket == tk {
                return Ok(argument_num);
            } else {
                return Err(self.parser_err_str("illegal argument list"));
            }
        }
    }
    #[inline]
    fn emit_get_symbol(&mut self, symbol: &IString, line: usize) -> Result<(), String> {
        match self.resolve(symbol, self.depth) {
            VarLoc::ThisRef => {
                self.emit_with_line(Instr::GetThis, line);
            }
            VarLoc::Local(x) => {
                self.emit_with_line(Instr::GetLocal(x), line);
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
    fn emit_set_symbol(&mut self, symbol: &IString, line: usize) -> Result<(), String> {
        match self.resolve(symbol, self.depth) {
            VarLoc::ThisRef => {
                return Err(self.parser_err_str("set `this` is invalid"));
            }
            VarLoc::Local(x) => {
                self.emit_with_line(Instr::SetLocal(x), line);
            }
            VarLoc::UpValue(x) => {
                self.emit_with_line(Instr::SetUpValue(x), line);
            }
            VarLoc::Global(x) => {
                self.emit_with_line(Instr::SetGlobal(x), line);
            }
            VarLoc::NotFound => {
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
        let line = self.get_line();
        self.add_local(&symbol)?;
        self.open_env();
        self.consume(Token::LParen)?;
        let mut para_num = 0;
        while let Token::Symbol(s) = self.peek_not_eof()? {
            self.advance();
            self.add_local(&s)?;
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
        let tk = self.peek_not_eof()?;
        if tk == Token::ThreeDots {
            // varidic parameters
            self.advance();
            self.chunk[self.depth].is_va = true;
        }
        self.consume(Token::RParen)?;
        self.consume(Token::LBrace)?;
        self.parse_stmt_list()?;
        self.consume(Token::RBrace)?;
        let mut chunk = self.close_env();
        chunk.parameter_num = para_num;
        self.chunk.last_mut().unwrap().chunks.push(chunk);
        self.emit(Instr::LoadChunk(
            self.chunk.last().unwrap().chunks.len() - 1,
        ));
        self.emit_set_symbol(&symbol, line)?;
        // self.emit(Instr::Pop);
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
                self.parse_rval_expr(PrattPrecedence::Lowest)?;
                self.add_local(&symbol)?;
                self.emit_set_symbol(&symbol, self.get_line())?;
                // self.emit(Instr::Pop);
                self.consume(Token::Semicolon)?;
                return Ok(());
            }
        }
        self.add_local(&symbol)?;
        self.consume(Token::Semicolon)?;
        Ok(())
    }
    fn parse_while(&mut self) -> Result<(), String> {
        self.consume(Token::While)?;
        self.consume(Token::LParen)?;
        let jumpback_point = self.chunk[self.depth].bytecodes.len();
        self.parse_rval_expr(PrattPrecedence::Lowest)?;
        let patch_point = self.chunk[self.depth].bytecodes.len();
        self.emit(Instr::Nop); // to be patched
        self.emit(Instr::Pop);
        self.consume(Token::RParen)?;
        self.consume(Token::LBrace)?;
        self.open_block();
        self.func_ctx_stack.last_mut().unwrap().loop_ctx = true;
        self.func_ctx_stack
            .last_mut()
            .unwrap()
            .loop_ctx_stack
            .push(LoopCtx::default());
        self.parse_stmt_list()?;
        let cur_loop_ctx = self
            .func_ctx_stack
            .last_mut()
            .unwrap()
            .loop_ctx_stack
            .pop()
            .unwrap();
        // patch continue points, jump to loop header
        for continue_point in cur_loop_ctx.continue_patch_point.iter() {
            self.chunk[self.depth].bytecodes[*continue_point] =
                Instr::Jump(jumpback_point as i32 - *continue_point as i32);
        }
        self.func_ctx_stack.last_mut().unwrap().loop_ctx = false;
        self.emit(Instr::Jump(
            jumpback_point as i32 - self.chunk[self.depth].bytecodes.len() as i32,
        ));
        self.close_block();
        self.consume(Token::RBrace)?;
        self.chunk[self.depth].bytecodes[patch_point] =
            Instr::JumpIfNot(self.chunk[self.depth].bytecodes.len() as i32 - patch_point as i32);
        self.emit(Instr::Pop);
        // patch break points, jump to end of loop
        for break_point in cur_loop_ctx.break_patch_point.iter() {
            self.chunk[self.depth].bytecodes[*break_point] =
                Instr::Jump(self.chunk[self.depth].bytecodes.len() as i32 - *break_point as i32);
        }

        Ok(())
    }
    fn parse_block(&mut self) -> Result<(), String> {
        self.consume(Token::LBrace)?;
        self.open_block();
        loop {
            let tk = self.peek_not_eof()?;
            if tk == Token::RBrace {
                break;
            }
            self.parse()?;
        }
        self.close_block();
        self.consume(Token::RBrace)?;
        Ok(())
    }

    fn parse_stmt_list(&mut self) -> Result<(), String> {
        loop {
            let tk = self.peek_not_eof()?;
            if tk == Token::RBrace {
                break;
            }
            self.parse()?;
        }
        Ok(())
    }
    fn parse_if(&mut self) -> Result<(), String> {
        self.consume(Token::If)?;
        self.consume(Token::LParen)?;
        self.parse_rval_expr(PrattPrecedence::Lowest)?;
        let patch_point = self.chunk[self.depth].bytecodes.len();
        // emit an empty slot, jump to FALSE branch but FALSE branch is now not parsed.
        self.emit(Instr::Nop);
        // pop the bool value
        // self.emit(Instr::Pop);
        self.consume(Token::RParen)?;
        // self.consume(Token::LBrace)?;
        // self.open_block();
        self.parse()?;
        let patch_point2 = self.chunk[self.depth].bytecodes.len();

        // emit an empty slot, jump to end of if statement, but we still don't know if there
        // is an else clause.
        self.emit(Instr::Nop);
        // self.close_block();
        // self.consume(Token::RBrace)?;
        self.chunk[self.depth].bytecodes[patch_point] =
            Instr::JumpIfNot((self.chunk[self.depth].bytecodes.len() - patch_point) as i32);
        if let Some(tok) = self.peek() {
            if Token::Else == tok {
                self.advance();
                self.parse()?;
                self.chunk[self.depth].bytecodes[patch_point2] =
                    Instr::Jump((self.chunk[self.depth].bytecodes.len() - patch_point2) as i32);
            }
        }
        // just leave the value on the stack, clean it at last
        self.emit(Instr::Pop);
        Ok(())
    }
    /// open function level env
    #[inline]
    fn open_env(&mut self) {
        self.chunk.push(Chunk::default());
        self.func_ctx_stack.push(FuncCtx::default());
        self.func_ctx_stack
            .last_mut()
            .unwrap()
            .block_ctx_stack
            .push(BlockCtx::default());
        self.depth += 1;
    }
    /// close function level env
    #[inline]
    fn close_env(&mut self) -> Chunk {
        self.emit_with_line(Instr::Return, 0);
        self.depth -= 1;
        self.func_ctx_stack.pop();
        self.chunk.pop().unwrap()
    }
    /// open block level env
    #[inline]
    fn open_block(&mut self) {
        self.func_ctx_stack
            .last_mut()
            .unwrap()
            .block_ctx_stack
            .push(BlockCtx::default());
    }
    /// close block level env
    #[inline]
    fn close_block(&mut self) {
        self.func_ctx_stack
            .last_mut()
            .unwrap()
            .block_ctx_stack
            .pop();
    }

    fn add_local(&mut self, symbol: &IString) -> Result<(), String> {
        if self.depth == 0 && self.func_ctx_stack[self.depth].block_ctx_stack.len() == 1 {
            // outermost scope => Global
            let s = symbol.clone();
            self.exported_symbols.push(s);
            return Ok(());
        }
        if self.func_ctx_stack[self.depth]
            .block_ctx_stack
            .last()
            .unwrap()
            .symbol_table
            .get(symbol)
            != None
        {
            return Err(self.parser_err_str("redeclaration of symbol"));
        }
        let num_locals = self.chunk[self.depth].num_locals;
        self.func_ctx_stack[self.depth]
            .block_ctx_stack
            .last_mut()
            .unwrap()
            .symbol_table
            .insert(symbol.clone(), num_locals);
        self.func_ctx_stack[self.depth]
            .block_ctx_stack
            .last_mut()
            .unwrap()
            .symbol_captured
            .push(false);
        self.chunk[self.depth].num_locals += 1;
        Ok(())
    }

    fn parse_assign_or_rval_expr(&mut self) -> Result<(), String> {
        if let Token::Symbol(s) = self.peek_not_eof()? {
            if !self.method_ctx && s.get_inner() == "this" {
                return Err(self.parser_err_str("`this` can NOT be used outside methods"));
            }
            match self.resolve(&s, self.depth) {
                VarLoc::Local(x) => self.emit(Instr::GetLocal(x)),
                VarLoc::UpValue(x) => self.emit(Instr::GetUpValue(x)),
                VarLoc::Global(x) => self.emit(Instr::GetGlobal(x)),
                VarLoc::ThisRef => self.emit(Instr::GetThis),
                _ => {
                    return Err(self.parser_err_str("symbol not found"));
                }
            }
            self.advance();
            let mut is_assign = true;
            while let Some(tk) = self.peek() {
                if tk.is_assign() {
                    break;
                } else if tk == Token::LBracket {
                    let line = self.get_line();
                    self.advance();
                    self.parse_rval_expr(PrattPrecedence::Lowest)?;
                    self.consume(Token::RBracket)?;
                    self.emit_with_line(Instr::GetCollection(0), line);
                    continue;
                } else if tk == Token::Dot {
                    let line = self.get_line();
                    self.advance();
                    if let Token::Symbol(s) = self.peek_not_eof()? {
                        self.load_value(Value::String(s));
                        self.advance();
                    } else {
                        return Err(self.parser_err_str("invalid rval expr"));
                    }
                    self.emit_with_line(Instr::GetCollection(1), line);
                    continue;
                } else {
                    is_assign = false;
                    break;
                }
            }
            if is_assign {
                // consume '='
                self.advance();
                // change get operation to corresponging set operation
                let last_instr = self.chunk[self.depth].bytecodes.pop().unwrap();
                self.parse_rval_expr(PrattPrecedence::Lowest)?;
                // now value is on the top of stack
                let modified_instr = match last_instr {
                    Instr::GetCollection(v) => {
                        // idx already on stack[top-1]
                        Instr::SetCollection(v)
                    }
                    Instr::GetLocal(x) => Instr::SetLocal(x),
                    Instr::GetGlobal(x) => Instr::SetGlobal(x),
                    Instr::GetUpValue(x) => Instr::SetUpValue(x),
                    _ => {
                        unreachable!()
                    }
                };
                self.emit(modified_instr);
            } else {
                // already parsed a symbol
                // no bracktrack
                self.parse_rval_expr2(PrattPrecedence::Lowest, true)?;
                self.emit(Instr::Pop);
            }
        } else {
            self.parse_rval_expr(PrattPrecedence::Lowest)?;
            self.emit(Instr::Pop);
        }
        Ok(())
    }

    pub fn parse_rval_expr(&mut self, prec: PrattPrecedence) -> Result<(), String> {
        self.parse_rval_expr2(prec, false)
    }

    /// [`maybe_assign`] indicates that we have already parsed a symbol
    pub fn parse_rval_expr2(
        &mut self,
        prec: PrattPrecedence,
        maybe_assign: bool,
    ) -> Result<(), String> {
        // Pratt Parser
        if !maybe_assign {
            match self.peek_not_eof()? {
                Token::Symbol(s) => {
                    if !self.method_ctx && s.get_inner() == "this" {
                        return Err(self.parser_err_str("`this` can NOT be used outside methods"));
                    }
                    match self.resolve(&s, self.depth) {
                        VarLoc::Local(x) => self.emit(Instr::GetLocal(x)),
                        VarLoc::UpValue(x) => self.emit(Instr::GetUpValue(x)),
                        VarLoc::Global(x) => self.emit(Instr::GetGlobal(x)),
                        VarLoc::ThisRef => self.emit(Instr::GetThis),
                        _ => {
                            return Err(self.parser_err_str("symbol not found"));
                        }
                    }
                    self.advance();
                }
                Token::Super => {
                    if !self.method_ctx {
                        return Err(self.parser_err_str("`super` can NOT be used outside methods"));
                    }
                    self.advance();
                    self.consume(Token::Dot)?;
                    if let Token::Symbol(s) = self.peek_not_eof()? {
                        self.emit(Instr::GetThis);
                        self.load_value(Value::String(s));
                        self.emit(Instr::GetSuperMethod);
                        self.advance();
                    } else {
                        return Err(self.parser_err_str("expect symbol after `super.`"));
                    }
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
                    self.advance();
                }

                Token::LBracket => {
                    self.parse_array()?;
                }
                Token::Dict => {
                    self.parse_dict()?;
                }
                Token::Sub => {
                    let line = self.token_cood[self.ptr].0;
                    self.advance();
                    self.parse_rval_expr(PrattPrecedence::Unary)?;
                    self.emit_with_line(Instr::Negate, line);
                }
                Token::Not => {
                    let line = self.token_cood[self.ptr].0;
                    self.advance();
                    self.parse_rval_expr(PrattPrecedence::Unary)?;
                    self.emit_with_line(Instr::Not, line);
                }
                Token::LParen => {
                    self.advance();
                    self.parse_rval_expr(PrattPrecedence::Lowest)?;
                    self.consume(Token::RParen)?;
                }
                Token::InterplotBegin => {
                    self.advance();
                    // require builtin string function
                    let f = self.string_pool.creat_istring("string");
                    self.emit_get_symbol(&f, self.get_line())?;

                    self.parse_rval_expr(PrattPrecedence::Lowest)?;
                    self.consume(Token::InterplotEnd)?;
                    // stack: [builtin string()] [rval value]
                    self.emit(Instr::Call(1));
                }
                Token::Stick => {
                    // lambda
                    self.advance();
                    self.open_env();
                    let mut para_num = 0;
                    while let Token::Symbol(s) = self.peek_not_eof()? {
                        self.advance();
                        self.add_local(&s)?;
                        para_num += 1;
                        match self.consume(Token::Comma) {
                            Ok(()) => {}
                            Err(_) => {
                                if Token::Stick == self.peek_not_eof()? {
                                    //self.advance();
                                    break;
                                } else {
                                    return Err(self
                                        .parser_err_str("expect `|` after lambda parameter list"));
                                }
                            }
                        }
                    }
                    self.consume(Token::Stick)?;
                    self.consume(Token::LBrace)?;
                    self.parse_stmt_list()?;
                    self.consume(Token::RBrace)?;
                    let mut chunk = self.close_env();
                    chunk.parameter_num = para_num;
                    self.chunk.last_mut().unwrap().chunks.push(chunk);
                    // evaluate to a closure
                    self.emit(Instr::LoadChunk(
                        self.chunk.last().unwrap().chunks.len() - 1,
                    ));
                }
                _ => {
                    //println!("{:?}", c);
                    return Ok(());
                    //unimplemented!();
                }
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
                self.parse_rval_expr(PrattPrecedence::Lowest)?;
                self.consume(Token::RBracket)?;
                self.emit_with_line(Instr::GetCollection(0), line);
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
                self.emit_with_line(Instr::GetCollection(1), line);
                continue;
            }
            let backpatch_point: usize = self.chunk[self.depth].bytecodes.len();
            let nprec = get_precedence(&tk);
            if nprec != PrattPrecedence::None {
                if get_precedence(&tk) <= prec {
                    break;
                }
                if tk == Token::And || tk == Token::Or {
                    self.emit(Instr::Nop);
                    // discard left value bool
                    self.emit(Instr::Pop);
                }
                self.advance();
                self.parse_rval_expr(nprec)?;
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
                    let pos = self.chunk[self.depth].bytecodes.len();
                    self.chunk[self.depth].bytecodes[backpatch_point] =
                        Instr::JumpIfNot((pos - backpatch_point) as i32);
                    self.chunk[self.depth].lines[backpatch_point] = line;
                }
                Token::Or => {
                    let pos = self.chunk[self.depth].bytecodes.len();
                    self.chunk[self.depth].bytecodes[backpatch_point] =
                        Instr::JumpIfTrue((pos - backpatch_point) as i32);
                    self.chunk[self.depth].lines[backpatch_point] = line;
                }
                Token::Dots => {
                    self.emit_with_line(Instr::MakeRange, line);
                }
                Token::DotsEq => {
                    self.emit_with_line(Instr::MakeRangeClosed, line);
                }
                Token::PipeOp => {
                    // `para |> functor` => `functor(para)`
                    self.emit_with_line(Instr::Swap2, line);
                    self.emit_with_line(Instr::Call(1), line);
                }
                Token::Is => {
                    self.emit_with_line(Instr::ClassIs, line);
                }
                _ => return Err(self.parser_err_str("infix operator required here.")),
            }
        }
        Ok(())
    }
    fn parse_array(&mut self) -> Result<(), String> {
        self.consume(Token::LBracket)?;
        let line = self.get_line();
        let arg_num = self.parse_argument()?;
        self.emit_with_line(Instr::InitArray(arg_num), line);
        self.consume(Token::RBracket)?;
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
            self.parse_rval_expr(PrattPrecedence::Lowest)?;
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
                self.advance();
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
            self.chunk[self.depth].lines.push(self.token_cood.last().unwrap().0);
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
    fn push_constant(&mut self, c: Value) -> usize {
        self.chunk[self.depth].constants.push(c);
        return self.chunk[self.depth].constants.len() - 1;
    }
    fn push_unique_number(&mut self, v: f64) -> usize {
        for (i, c) in self.chunk[self.depth].constants.iter().enumerate() {
            if let Value::Number(v1) = c {
                if (v - v1).abs() < f64::EPSILON {
                    return i;
                }
            }
        }
        self.chunk[self.depth].constants.push(Value::Number(v));
        return self.chunk[self.depth].constants.len() - 1;
    }

    fn push_unique_string(&mut self, s: &IString) -> usize {
        for (i, c) in self.chunk[self.depth].constants.iter().enumerate() {
            if let Value::String(s1) = c {
                if s == s1 {
                    return i;
                }
            }
        }
        self.chunk[self.depth]
            .constants
            .push(Value::String(s.clone()));
        return self.chunk[self.depth].constants.len() - 1;
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
            let idx = self.push_constant(v);
            self.emit(Instr::Load(idx));
        }
    }

    fn resolve(&mut self, symbol: &IString, depth: usize) -> VarLoc {
        if symbol.get_inner() == "this" {
            if !self.method_ctx {
                return VarLoc::NotFound;
            }
            return VarLoc::ThisRef;
        }
        // from innermost Block to outermost Block of current Function
        for ctx in self.func_ctx_stack[depth].block_ctx_stack.iter().rev() {
            if let Some(x) = ctx.symbol_table.get(symbol) {
                return VarLoc::Local(*x);
            }
        }
        if depth == 0 {
            // stacktop = GLOBAL[constant[idx]]
            let idx = self.push_unique_string(symbol);
            return VarLoc::Global(idx);
        }
        if let Some(x) = self.chunk[depth].upvalues.iter().position(|upv| match upv {
            UpValueDecl::Ref(_, s) => s == symbol,
            UpValueDecl::RefUpValue(_, s) => s == symbol,
        }) {
            return VarLoc::UpValue(x);
        }
        let ret = self.resolve(symbol, depth - 1);
        match ret {
            VarLoc::Local(x) => {
                // local variable of outer function
                // add to upvalue list of this function
                self.chunk[depth]
                    .upvalues
                    .push(UpValueDecl::Ref(x, symbol.clone()));
                return VarLoc::UpValue(self.chunk[depth].upvalues.len() - 1);
            }
            VarLoc::UpValue(x) => {
                // upvalue of outer function
                self.chunk[depth]
                    .upvalues
                    .push(UpValueDecl::RefUpValue(x, symbol.clone()));
                return VarLoc::UpValue(self.chunk[depth].upvalues.len() - 1);
            }
            VarLoc::Global(x) => {
                // Global variable
                return VarLoc::Global(x);
            }
            _ => {
                return VarLoc::NotFound;
            }
        }
    }

    #[inline]
    fn get_line(&self) -> usize {
        if self.ptr >= self.token_cood.len() {
            self.token_cood.last().unwrap().0
        } else {
            self.token_cood[self.ptr].0
        }
    }
}
#[cfg(test)]
mod test {}
