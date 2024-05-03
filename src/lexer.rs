use std::str::Chars;

pub const C_LINE_FEED: char = '\n';
pub const C_CARRIAGE_RETURN: char = '\r';
pub const C_FORM_FEED: char = '\u{c}';

pub const C_TAB: char = '\t';
pub const C_SPACE: char = ' ';

pub const C_SOLIDUS: char = '/';
pub const C_REVERSE_SOLIDUS: char = '\\';
pub const C_ASTERISK: char = '*';

pub const C_LEFT_PARENTHESIS: char = '(';
pub const C_RIGHT_PARENTHESIS: char = ')';
pub const C_LEFT_CURLY: char = '{';
pub const C_RIGHT_CURLY: char = '}';
pub const C_LEFT_SQUARE: char = '[';
pub const C_RIGHT_SQUARE: char = ']';

pub const C_QUOTATION_MARK: char = '"';
pub const C_APOSTROPHE: char = '\'';

pub const C_FULL_STOP: char = '.';
pub const C_COLON: char = ':';
pub const C_SEMICOLON: char = ';';
pub const C_COMMA: char = ',';
pub const C_PERCENTAGE: char = '%';
pub const C_AT_SIGN: char = '@';

pub const C_LOW_LINE: char = '_';
pub const C_LOWER_A: char = 'a';
pub const C_LOWER_E: char = 'e';
pub const C_LOWER_F: char = 'f';
pub const C_LOWER_Z: char = 'z';
pub const C_UPPER_A: char = 'A';
pub const C_UPPER_E: char = 'E';
pub const C_UPPER_F: char = 'F';
pub const C_UPPER_Z: char = 'Z';
pub const C_0: char = '0';
pub const C_9: char = '9';

pub const C_NUMBER_SIGN: char = '#';
pub const C_PLUS_SIGN: char = '+';
pub const C_HYPHEN_MINUS: char = '-';

pub const C_LESS_THAN_SIGN: char = '<';
pub const C_GREATER_THAN_SIGN: char = '>';

pub type Pos = u32;

pub trait Visitor<'s> {
    fn function(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn ident(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn url(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: Pos,
        end: Pos,
        content_start: Pos,
        content_end: Pos,
    ) -> Option<()>;
    fn string(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn is_selector(&mut self, lexer: &mut Lexer<'s>) -> Option<bool>;
    fn id(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn left_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn right_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn comma(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn class(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn pseudo_function(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn pseudo_class(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn semicolon(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn at_keyword(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn left_curly_bracket(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
    fn right_curly_bracket(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()>;
}

#[derive(Debug, Clone)]
pub struct Lexer<'s> {
    value: &'s str,
    iter: Chars<'s>,
    cur_pos: Option<Pos>,
    cur: Option<char>,
    peek: Option<char>,
    peek2: Option<char>,
}

impl<'s> From<&'s str> for Lexer<'s> {
    fn from(value: &'s str) -> Self {
        let mut iter = value.chars();
        let peek = iter.next();
        let peek2 = iter.next();
        Self {
            value,
            iter,
            cur_pos: None,
            cur: None,
            peek,
            peek2,
        }
    }
}

impl<'s> Lexer<'s> {
    #[must_use]
    pub fn consume(&mut self) -> Option<char> {
        self.cur_pos = self.peek_pos();
        self.cur = self.peek;
        self.peek = self.peek2;
        self.peek2 = self.iter.next();
        self.cur()
    }

    pub fn cur_pos(&self) -> Option<Pos> {
        self.cur_pos
    }

    pub fn cur(&self) -> Option<char> {
        self.cur
    }

    pub fn peek_pos(&self) -> Option<Pos> {
        if let Some(pos) = self.cur_pos() {
            self.cur().map(|c| pos + c.len_utf8() as u32)
        } else {
            Some(0)
        }
    }

    pub fn peek(&self) -> Option<char> {
        self.peek
    }

    pub fn peek2_pos(&self) -> Option<Pos> {
        self.peek_pos()
            .and_then(|pos| self.peek().map(|c| pos + c.len_utf8() as u32))
    }

    pub fn peek2(&self) -> Option<char> {
        self.peek2
    }

    pub fn slice(&self, start: Pos, end: Pos) -> Option<&'s str> {
        self.value.get(start as usize..end as usize)
    }
}

impl<'s> Lexer<'s> {
    pub fn lex<T: Visitor<'s>>(&mut self, visitor: &mut T) {
        self.lex_impl(visitor);
    }

    fn lex_impl<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        while self.cur().is_some() {
            self.consume_comments()?;
            let c = self.cur()?;
            if c < '\u{80}' {
                // https://drafts.csswg.org/css-syntax/#consume-token
                match c {
                    c if is_white_space(c) => self.consume_space()?,
                    C_QUOTATION_MARK => self.consume_string(visitor, C_QUOTATION_MARK)?,
                    C_NUMBER_SIGN => self.consume_number_sign(visitor)?,
                    C_APOSTROPHE => self.consume_string(visitor, C_APOSTROPHE)?,
                    C_LEFT_PARENTHESIS => self.consume_left_parenthesis(visitor)?,
                    C_RIGHT_PARENTHESIS => self.consume_right_parenthesis(visitor)?,
                    C_PLUS_SIGN => self.consume_plus_sign()?,
                    C_COMMA => self.consume_comma(visitor)?,
                    C_HYPHEN_MINUS => self.consume_minus(visitor)?,
                    C_FULL_STOP => self.consume_full_stop(visitor)?,
                    C_COLON => self.consume_potential_pseudo(visitor)?,
                    C_SEMICOLON => self.consume_semicolon(visitor)?,
                    C_LESS_THAN_SIGN => self.consume_less_than_sign()?,
                    C_AT_SIGN => self.consume_at_sign(visitor)?,
                    C_LEFT_SQUARE => self.consume_delim()?,
                    C_REVERSE_SOLIDUS => self.consume_reverse_solidus(visitor)?,
                    C_RIGHT_SQUARE => self.consume_delim()?,
                    C_LEFT_CURLY => self.consume_left_curly(visitor)?,
                    C_RIGHT_CURLY => self.consume_right_curly(visitor)?,
                    c if is_digit(c) => self.consume_numeric_token()?,
                    c if is_ident_start(c) => self.consume_ident_like(visitor)?,
                    _ => self.consume_delim()?,
                }
            }
        }
        Some(())
    }

    pub fn consume_comments(&mut self) -> Option<()> {
        if self.cur()? == C_SOLIDUS && self.peek()? == C_ASTERISK {
            while let Some(c) = self.consume() {
                if c == C_ASTERISK && self.peek()? == C_SOLIDUS {
                    self.consume()?;
                    self.consume()?;
                    break;
                }
            }
        }
        Some(())
    }

    pub fn consume_delim(&mut self) -> Option<()> {
        self.consume()?;
        Some(())
    }

    pub fn consume_space(&mut self) -> Option<()> {
        while is_white_space(self.consume()?) {}
        Some(())
    }

    pub fn consume_numeric_token(&mut self) -> Option<()> {
        self.consume_number()?;
        let c = self.cur()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            return self.consume_ident_sequence();
        }
        if c == C_PERCENTAGE {
            self.consume()?;
        }
        Some(())
    }

    pub fn consume_number(&mut self) -> Option<()> {
        while matches!(self.consume(), Some(c) if is_digit(c)) {}
        if self.cur()? == C_FULL_STOP && is_digit(self.peek()?) {
            self.consume()?;
            while matches!(self.consume(), Some(c) if is_digit(c)) {}
        }
        let c = self.cur()?;
        if c == C_LOWER_E || c == C_UPPER_E {
            let c = self.peek()?;
            if is_digit(c) {
                self.consume()?;
            } else if c == C_HYPHEN_MINUS || c == C_PLUS_SIGN {
                let c = self.peek2()?;
                if is_digit(c) {
                    self.consume()?;
                    self.consume()?;
                } else {
                    return Some(());
                }
            } else {
                return Some(());
            }
        } else {
            return Some(());
        }
        while matches!(self.consume(), Some(c) if is_digit(c)) {}
        Some(())
    }

    pub fn consume_ident_sequence(&mut self) -> Option<()> {
        loop {
            let c = self.cur()?;
            if maybe_valid_escape(c) {
                self.consume()?;
                self.consume_escaped()?;
            } else if is_ident(c) {
                self.consume()?;
            } else {
                return Some(());
            }
        }
    }

    pub fn consume_escaped(&mut self) -> Option<()> {
        if is_hex_digit(self.cur()?) {
            for _ in 1..5 {
                if !is_hex_digit(self.consume()?) {
                    break;
                }
            }
            if is_white_space(self.cur()?) {
                self.consume()?;
            }
        } else {
            self.consume()?;
        }
        Some(())
    }

    pub fn consume_ident_like<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume_ident_sequence()?;
        let peek_pos = self.peek_pos()?;
        if self.cur_pos()? == start + 3
            && self.slice(start, peek_pos)?.to_ascii_lowercase() == "url("
        {
            while is_white_space(self.consume()?) {}
            let c = self.cur()?;
            if c == C_QUOTATION_MARK || c == C_APOSTROPHE {
                visitor.function(self, start, peek_pos)
            } else {
                self.consume_url(visitor, start)
            }
        } else if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume()?;
            visitor.function(self, start, self.cur_pos()?)
        } else {
            visitor.ident(self, start, self.cur_pos()?)
        }
    }

    pub fn consume_url<T: Visitor<'s>>(
        self: &mut Lexer<'s>,
        visitor: &mut T,
        start: Pos,
    ) -> Option<()> {
        let content_start = self.cur_pos()?;
        loop {
            let c = self.cur()?;
            if maybe_valid_escape(c) {
                self.consume()?;
                self.consume_escaped()?;
            } else if is_white_space(c) {
                let content_end = self.cur_pos()?;
                while is_white_space(self.consume()?) {}
                if self.cur()? != C_RIGHT_PARENTHESIS {
                    return Some(());
                }
                self.consume()?;
                return visitor.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_RIGHT_PARENTHESIS {
                let content_end = self.cur_pos()?;
                self.consume()?;
                return visitor.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_LEFT_PARENTHESIS {
                return Some(());
            } else {
                self.consume()?;
            }
        }
    }

    pub fn consume_string<T: Visitor<'s>>(&mut self, visitor: &mut T, end: char) -> Option<()> {
        let start = self.cur_pos()?;
        while let Some(c) = self.consume() {
            if c == end {
                self.consume()?;
                break;
            }
            if is_new_line(c) {
                break;
            }
            if c == C_REVERSE_SOLIDUS {
                let c2 = self.consume()?;
                if is_new_line(c2) {
                    self.consume()?;
                } else if are_valid_escape(c, c2) {
                    self.consume_escaped()?;
                }
            }
        }
        visitor.string(self, start, self.cur_pos()?)
    }

    pub fn consume_number_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c2 = self.peek()?;
        if is_ident(c2) || are_valid_escape(c2, self.peek2()?) {
            let start = self.cur_pos()?;
            let c = self.consume()?;
            if visitor.is_selector(self)? && start_ident_sequence(c, self.peek()?, self.peek2()?) {
                self.consume_ident_sequence()?;
                return visitor.id(self, start, self.cur_pos()?);
            }
        } else {
            return self.consume_delim();
        }
        Some(())
    }

    pub fn consume_left_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.left_parenthesis(self, end - 1, end)
    }

    pub fn consume_right_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.right_parenthesis(self, end - 1, end)
    }

    pub fn consume_plus_sign(&mut self) -> Option<()> {
        if start_number(self.cur()?, self.peek()?, self.peek2()?) {
            self.consume_numeric_token()
        } else {
            self.consume_delim()
        }
    }

    pub fn consume_comma<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.comma(self, end - 1, end)
    }

    pub fn consume_minus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            self.consume_numeric_token()
        } else if c2 == C_HYPHEN_MINUS && c3 == C_GREATER_THAN_SIGN {
            self.consume()?;
            self.consume()?;
            Some(())
        } else if start_ident_sequence(c, c2, c3) {
            self.consume_ident_like(visitor)
        } else {
            self.consume_delim()
        }
    }

    pub fn consume_full_stop<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            return self.consume_numeric_token();
        }
        let start = self.cur_pos()?;
        self.consume()?;
        if !visitor.is_selector(self)? || !start_ident_sequence(c2, c3, self.peek2()?) {
            return self.consume_delim();
        }
        self.consume_ident_sequence()?;
        visitor.class(self, start, self.cur_pos()?)
    }

    pub fn consume_potential_pseudo<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        let c = self.consume()?;
        if !visitor.is_selector(self)? || !start_ident_sequence(c, self.peek()?, self.peek2()?) {
            return Some(());
        }
        self.consume_ident_sequence()?;
        if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume()?;
            visitor.pseudo_function(self, start, self.cur_pos()?)
        } else {
            visitor.pseudo_class(self, start, self.cur_pos()?)
        }
    }

    pub fn consume_semicolon<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.semicolon(self, end - 1, end)
    }

    pub fn consume_less_than_sign(&mut self) -> Option<()> {
        if self.consume()? == '!' && self.peek()? == '-' && self.peek2()? == '-' {
            self.consume()?;
            self.consume()?;
            self.consume()?;
        }
        Some(())
    }

    pub fn consume_at_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        let c = self.consume()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            self.consume_ident_sequence()?;
            return visitor.at_keyword(self, start, self.cur_pos()?);
        }
        Some(())
    }

    pub fn consume_reverse_solidus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        if are_valid_escape(self.cur()?, self.peek()?) {
            self.consume_ident_like(visitor)
        } else {
            self.consume_delim()
        }
    }

    pub fn consume_left_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.left_curly_bracket(self, end - 1, end)
    }

    pub fn consume_right_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.right_curly_bracket(self, end - 1, end)
    }
}

impl Lexer<'_> {
    pub fn consume_white_space_and_comments(&mut self) -> Option<()> {
        loop {
            self.consume_comments()?;
            if is_white_space(self.cur()?) {
                self.consume_space()?;
            } else {
                break;
            }
        }
        Some(())
    }

    // pub fn eat_while_line(&mut self) -> Option<()> {
    //     loop {
    //         let c = self.cur()?;
    //         if is_space(c) {
    //             self.consume()?;
    //             continue;
    //         }
    //         if is_new_line(c) {
    //             self.consume()?;
    //         }
    //         // For \r\n
    //         if self.cur()? == C_CARRIAGE_RETURN && self.peek()? == C_LINE_FEED {
    //             self.consume()?;
    //         }
    //         break;
    //     }
    //     Some(())
    // }
}

pub fn is_new_line(c: char) -> bool {
    c == C_LINE_FEED || c == C_CARRIAGE_RETURN || c == C_FORM_FEED
}

pub fn is_space(c: char) -> bool {
    c == C_TAB || c == C_SPACE
}

pub fn is_white_space(c: char) -> bool {
    is_new_line(c) || is_space(c)
}

pub fn is_digit(c: char) -> bool {
    c >= C_0 && c <= C_9
}

pub fn is_hex_digit(c: char) -> bool {
    is_digit(c) || (c >= C_UPPER_A && c <= C_UPPER_F) || (c >= C_LOWER_A && c <= C_LOWER_F)
}

pub fn is_ident_start(c: char) -> bool {
    c == C_LOW_LINE
        || (c >= C_LOWER_A && c <= C_LOWER_Z)
        || (c >= C_UPPER_A && c <= C_UPPER_Z)
        || c > '\u{80}'
}

pub fn is_ident(c: char) -> bool {
    is_ident_start(c) || is_digit(c) || c == C_HYPHEN_MINUS
}

pub fn start_ident_sequence(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_HYPHEN_MINUS {
        is_ident_start(c2) || c2 == C_HYPHEN_MINUS || are_valid_escape(c2, c3)
    } else {
        is_ident_start(c1) || are_valid_escape(c1, c2)
    }
}

pub fn maybe_valid_escape(c: char) -> bool {
    c == C_REVERSE_SOLIDUS
}

pub fn are_valid_escape(c1: char, c2: char) -> bool {
    c1 == C_REVERSE_SOLIDUS && !is_new_line(c2)
}

pub fn start_number(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_PLUS_SIGN || c1 == C_HYPHEN_MINUS {
        is_digit(c2) || (c2 == C_FULL_STOP && is_digit(c3))
    } else {
        is_digit(c1) || (c1 == C_FULL_STOP && is_digit(c2))
    }
}
