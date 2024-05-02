use std::str::Chars;

const C_LINE_FEED: char = '\n';
const C_CARRIAGE_RETURN: char = '\r';
const C_FORM_FEED: char = '\u{c}';

const C_TAB: char = '\t';
const C_SPACE: char = ' ';

const C_SOLIDUS: char = '/';
const C_REVERSE_SOLIDUS: char = '\\';
const C_ASTERISK: char = '*';

const C_LEFT_PARENTHESIS: char = '(';
const C_RIGHT_PARENTHESIS: char = ')';
const C_LEFT_CURLY: char = '{';
const C_RIGHT_CURLY: char = '}';
const C_LEFT_SQUARE: char = '[';
const C_RIGHT_SQUARE: char = ']';

const C_QUOTATION_MARK: char = '"';
const C_APOSTROPHE: char = '\'';

const C_FULL_STOP: char = '.';
const C_COLON: char = ':';
const C_SEMICOLON: char = ';';
const C_COMMA: char = ',';
const C_PERCENTAGE: char = '%';
const C_AT_SIGN: char = '@';

const C_LOW_LINE: char = '_';
const C_LOWER_A: char = 'a';
const C_LOWER_E: char = 'e';
const C_LOWER_F: char = 'f';
const C_LOWER_Z: char = 'z';
const C_UPPER_A: char = 'A';
const C_UPPER_E: char = 'E';
const C_UPPER_F: char = 'F';
const C_UPPER_Z: char = 'Z';
const C_0: char = '0';
const C_9: char = '9';

const C_NUMBER_SIGN: char = '#';
const C_PLUS_SIGN: char = '+';
const C_HYPHEN_MINUS: char = '-';

const C_LESS_THAN_SIGN: char = '<';
const C_GREATER_THAN_SIGN: char = '>';

pub trait Visitor<'s> {
    fn function(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn ident(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn url(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: usize,
        end: usize,
        content_start: usize,
        content_end: usize,
    ) -> Option<()>;
    fn string(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn is_selector(&mut self, lexer: &mut Lexer<'s>) -> Option<bool>;
    fn id(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn left_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn right_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn comma(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn class(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn pseudo_function(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn pseudo_class(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn semicolon(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn at_keyword(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()>;
    fn left_curly_bracket(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize)
        -> Option<()>;
    fn right_curly_bracket(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: usize,
        end: usize,
    ) -> Option<()>;
}

#[derive(Debug, Clone)]
pub struct Lexer<'s> {
    value: &'s str,
    iter: Chars<'s>,
    cur_pos: Option<usize>,
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

    pub fn cur_pos(&self) -> Option<usize> {
        self.cur_pos
    }

    pub fn cur(&self) -> Option<char> {
        self.cur
    }

    pub fn peek_pos(&self) -> Option<usize> {
        if let Some(pos) = self.cur_pos() {
            if let Some(c) = self.cur() {
                Some(pos + c.len_utf8())
            } else {
                None
            }
        } else {
            Some(0)
        }
    }

    pub fn peek(&self) -> Option<char> {
        self.peek
    }

    pub fn peek2_pos(&self) -> Option<usize> {
        if let Some(pos) = self.peek_pos() {
            if let Some(c) = self.peek() {
                Some(pos + c.len_utf8())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn peek2(&self) -> Option<char> {
        self.peek2
    }

    pub fn slice(&self, start: usize, end: usize) -> Option<&'s str> {
        self.value.get(start..end)
    }
}

impl<'s> Lexer<'s> {
    pub fn lex<T: Visitor<'s>>(&mut self, visitor: &mut T) {
        self.lex_impl(visitor);
    }

    fn lex_impl<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        while !self.cur().is_none() {
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

    fn consume_comments(&mut self) -> Option<()> {
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

    fn consume_delim(&mut self) -> Option<()> {
        self.consume()?;
        Some(())
    }

    fn consume_space(&mut self) -> Option<()> {
        while is_white_space(self.consume()?) {}
        Some(())
    }

    fn consume_numeric_token(&mut self) -> Option<()> {
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

    fn consume_number(&mut self) -> Option<()> {
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

    fn consume_ident_sequence(&mut self) -> Option<()> {
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

    fn consume_escaped(&mut self) -> Option<()> {
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

    fn consume_ident_like<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
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

    fn consume_url<T: Visitor<'s>>(
        self: &mut Lexer<'s>,
        visitor: &mut T,
        start: usize,
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

    fn consume_string<T: Visitor<'s>>(&mut self, visitor: &mut T, end: char) -> Option<()> {
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

    fn consume_number_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
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

    fn consume_left_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.left_parenthesis(self, end - 1, end)
    }

    fn consume_right_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.right_parenthesis(self, end - 1, end)
    }

    fn consume_plus_sign(&mut self) -> Option<()> {
        if start_number(self.cur()?, self.peek()?, self.peek2()?) {
            self.consume_numeric_token()
        } else {
            self.consume_delim()
        }
    }

    fn consume_comma<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.comma(self, end - 1, end)
    }

    fn consume_minus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
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

    fn consume_full_stop<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
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

    fn consume_potential_pseudo<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
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

    fn consume_semicolon<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.semicolon(self, end - 1, end)
    }

    fn consume_less_than_sign(&mut self) -> Option<()> {
        if self.consume()? == '!' && self.peek()? == '-' && self.peek2()? == '-' {
            self.consume()?;
            self.consume()?;
            self.consume()?;
        }
        Some(())
    }

    fn consume_at_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        let c = self.consume()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            self.consume_ident_sequence()?;
            return visitor.at_keyword(self, start, self.cur_pos()?);
        }
        Some(())
    }

    fn consume_reverse_solidus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        if are_valid_escape(self.cur()?, self.peek()?) {
            self.consume_ident_like(visitor)
        } else {
            self.consume_delim()
        }
    }

    fn consume_left_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.left_curly_bracket(self, end - 1, end)
    }

    fn consume_right_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        visitor.right_curly_bracket(self, end - 1, end)
    }
}

impl Lexer<'_> {
    pub fn eat_white_space_and_comments(&mut self) -> Option<()> {
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

fn is_new_line(c: char) -> bool {
    c == C_LINE_FEED || c == C_CARRIAGE_RETURN || c == C_FORM_FEED
}

fn is_space(c: char) -> bool {
    c == C_TAB || c == C_SPACE
}

fn is_white_space(c: char) -> bool {
    is_new_line(c) || is_space(c)
}

fn is_digit(c: char) -> bool {
    c >= C_0 && c <= C_9
}

fn is_hex_digit(c: char) -> bool {
    is_digit(c) || (c >= C_UPPER_A && c <= C_UPPER_F) || (c >= C_LOWER_A && c <= C_LOWER_F)
}

fn is_ident_start(c: char) -> bool {
    c == C_LOW_LINE
        || (c >= C_LOWER_A && c <= C_LOWER_Z)
        || (c >= C_UPPER_A && c <= C_UPPER_Z)
        || c > '\u{80}'
}

fn is_ident(c: char) -> bool {
    is_ident_start(c) || is_digit(c) || c == C_HYPHEN_MINUS
}

fn start_ident_sequence(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_HYPHEN_MINUS {
        is_ident_start(c2) || c2 == C_HYPHEN_MINUS || are_valid_escape(c2, c3)
    } else {
        is_ident_start(c1) || are_valid_escape(c1, c2)
    }
}

fn maybe_valid_escape(c: char) -> bool {
    c == C_REVERSE_SOLIDUS
}

fn are_valid_escape(c1: char, c2: char) -> bool {
    c1 == C_REVERSE_SOLIDUS && !is_new_line(c2)
}

fn start_number(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_PLUS_SIGN || c1 == C_HYPHEN_MINUS {
        is_digit(c2) || (c2 == C_FULL_STOP && is_digit(c3))
    } else {
        is_digit(c1) || (c1 == C_FULL_STOP && is_digit(c2))
    }
}

#[derive(Debug)]
enum Scope<'s> {
    TopLevel,
    InBlock,
    InAtImport(ImportData<'s>),
    AtImportInvalid,
    AtNamespaceInvalid,
}

#[derive(Debug)]
struct ImportData<'s> {
    start: usize,
    url: Option<&'s str>,
    url_range: Option<Range>,
    supports: ImportDataSupports<'s>,
    layer: ImportDataLayer<'s>,
}

impl ImportData<'_> {
    pub fn new(start: usize) -> Self {
        Self {
            start,
            url: None,
            url_range: None,
            supports: ImportDataSupports::None,
            layer: ImportDataLayer::None,
        }
    }

    pub fn in_supports(&self) -> bool {
        matches!(self.supports, ImportDataSupports::InSupports { .. })
    }

    pub fn layer_range(&self) -> Option<&Range> {
        let ImportDataLayer::EndLayer { range, .. } = &self.layer else {
            return None;
        };
        Some(range)
    }

    pub fn supports_range(&self) -> Option<&Range> {
        let ImportDataSupports::EndSupports { range, .. } = &self.supports else {
            return None;
        };
        Some(range)
    }
}

#[derive(Debug)]
enum ImportDataSupports<'s> {
    None,
    InSupports { start: usize },
    EndSupports { value: &'s str, range: Range },
}

#[derive(Debug)]
enum ImportDataLayer<'s> {
    None,
    EndLayer { value: &'s str, range: Range },
}

#[derive(Debug)]
struct BalancedItem {
    kind: BalancedItemKind,
    range: Range,
}

impl BalancedItem {
    pub fn new(name: &str, start: usize, end: usize) -> Self {
        Self {
            kind: BalancedItemKind::new(name),
            range: Range::new(start, end),
        }
    }

    pub fn new_other(start: usize, end: usize) -> Self {
        Self {
            kind: BalancedItemKind::Other,
            range: Range::new(start, end),
        }
    }
}

#[derive(Debug)]
enum BalancedItemKind {
    Url,
    ImageSet,
    Layer,
    Supports,
    Local,
    Global,
    Other,
}

impl BalancedItemKind {
    pub fn new(name: &str) -> Self {
        match name {
            "url" => Self::Url,
            "image-set" => Self::ImageSet,
            "layer" => Self::Layer,
            "supports" => Self::Supports,
            ":local" => Self::Local,
            ":global" => Self::Global,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug)]
enum CssModulesMode {
    Local,
    Global,
    None,
}

#[derive(Debug)]
pub struct CssModulesModeData {
    default: CssModulesMode,
    current: CssModulesMode,
}

impl CssModulesModeData {
    pub fn new(local: bool) -> Self {
        Self {
            default: if local {
                CssModulesMode::Local
            } else {
                CssModulesMode::Global
            },
            current: CssModulesMode::None,
        }
    }

    pub fn is_local_mode(&self) -> bool {
        match self.current {
            CssModulesMode::Local => true,
            CssModulesMode::Global => false,
            CssModulesMode::None => match self.default {
                CssModulesMode::Local => true,
                CssModulesMode::Global => false,
                CssModulesMode::None => false,
            },
        }
    }

    pub fn set_local(&mut self) {
        self.current = CssModulesMode::Local;
    }

    pub fn set_global(&mut self) {
        self.current = CssModulesMode::Global;
    }

    pub fn set_none(&mut self) {
        self.current = CssModulesMode::None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dependency<'s> {
    Url {
        request: &'s str,
        range: Range,
        kind: UrlRangeKind,
    },
    Import {
        request: &'s str,
        range: Range,
        layer: Option<&'s str>,
        supports: Option<&'s str>,
        media: Option<&'s str>,
    },
    Replace {
        content: &'s str,
        range: Range,
    },
    Local {
        name: &'s str,
        range: Range,
        kind: LocalKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UrlRangeKind {
    Function,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalKind {
    Ident,
    Var,
}

#[derive(Debug, Clone)]
pub enum Warning {
    Unexpected { unexpected: Range, range: Range },
    DuplicateUrl { range: Range },
    NamespaceNotSupportedInBundledCss { range: Range },
    NotPrecededAtImport { range: Range },
    ExpectedUrl { range: Range },
    ExpectedBefore { should_after: Range, range: Range },
}

pub fn lex_css_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(handle_dependency, handle_warning, None);
    lexer.lex(&mut visitor);
}

pub fn collect_css_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(true)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_global_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(false)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_global_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_global_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

#[derive(Debug)]
pub struct LexDependencies<'s, D, W> {
    mode_data: Option<CssModulesModeData>,
    scope: Scope<'s>,
    block_nesting_level: u32,
    allow_import_at_rule: bool,
    balanced: Vec<BalancedItem>,
    is_next_rule_prelude: bool,
    handle_dependency: D,
    handle_warning: W,
}

impl<'s, D: FnMut(Dependency<'s>), W: FnMut(Warning)> LexDependencies<'s, D, W> {
    pub fn new(
        handle_dependency: D,
        handle_warning: W,
        mode_data: Option<CssModulesModeData>,
    ) -> Self {
        Self {
            mode_data,
            scope: Scope::TopLevel,
            block_nesting_level: 0,
            allow_import_at_rule: true,
            balanced: Vec::new(),
            is_next_rule_prelude: true,
            handle_dependency,
            handle_warning,
        }
    }

    pub fn is_next_nested_syntax(&self, lexer: &Lexer) -> Option<bool> {
        let mut lexer = lexer.clone();
        lexer.eat_white_space_and_comments()?;
        let c = lexer.cur()?;
        if c == C_LEFT_CURLY {
            return Some(false);
        }
        Some(!is_ident_start(c))
    }

    pub fn get_media(&self, lexer: &Lexer<'s>, start: usize, end: usize) -> Option<&'s str> {
        let media = lexer.slice(start, end)?;
        let mut media_lexer = Lexer::from(media);
        media_lexer.consume()?;
        media_lexer.eat_white_space_and_comments()?;
        Some(media)
    }

    pub fn lex_export(&mut self, lexer: &mut Lexer<'s>, start: usize) -> Option<()> {
        lexer.eat_white_space_and_comments()?;
        let c = lexer.cur()?;
        if c != C_LEFT_CURLY {
            let end = lexer.peek_pos()?;
            (self.handle_warning)(Warning::Unexpected {
                unexpected: Range::new(lexer.cur_pos()?, end),
                range: Range::new(start, end),
            });
            return Some(());
        }
        lexer.consume()?;
        lexer.eat_white_space_and_comments()?;
        while lexer.cur()? != C_RIGHT_CURLY {
            lexer.eat_white_space_and_comments()?;
            let prop_start = lexer.cur_pos()?;
            loop {
                let c = lexer.cur()?;
                if c == C_COLON
                    || c == C_RIGHT_CURLY
                    || c == C_SEMICOLON
                    || (c == C_SOLIDUS && lexer.peek()? == C_ASTERISK)
                {
                    break;
                }
                lexer.consume()?;
            }
            let prop_end = lexer.cur_pos()?;
            lexer.eat_white_space_and_comments()?;
            if lexer.cur()? != C_COLON {
                let end = lexer.peek_pos()?;
                (self.handle_warning)(Warning::Unexpected {
                    unexpected: Range::new(lexer.cur_pos()?, end),
                    range: Range::new(prop_start, end),
                });
                return Some(());
            }
        }
        Some(())
    }
}

impl<'s, D: FnMut(Dependency<'s>), W: FnMut(Warning)> Visitor<'s> for LexDependencies<'s, D, W> {
    fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
        Some(self.is_next_rule_prelude)
    }

    fn url(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: usize,
        end: usize,
        content_start: usize,
        content_end: usize,
    ) -> Option<()> {
        let value = lexer.slice(content_start, content_end)?;
        match self.scope {
            Scope::InAtImport(ref mut import_data) => {
                if import_data.in_supports() {
                    return Some(());
                }
                if import_data.url.is_some() {
                    (self.handle_warning)(Warning::DuplicateUrl {
                        range: Range::new(import_data.start, end),
                    });
                    return Some(());
                }
                import_data.url = Some(value);
                import_data.url_range = Some(Range::new(start, end));
            }
            Scope::InBlock => (self.handle_dependency)(Dependency::Url {
                request: value,
                range: Range::new(start, end),
                kind: UrlRangeKind::Function,
            }),
            _ => {}
        }
        Some(())
    }

    fn string(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        match self.scope {
            Scope::InAtImport(ref mut import_data) => {
                let inside_url = matches!(
                    self.balanced.last(),
                    Some(last) if matches!(last.kind, BalancedItemKind::Url)
                );

                // Do not parse URLs in `supports(...)` and other strings if we already have a URL
                if import_data.in_supports() || (!inside_url && import_data.url.is_some()) {
                    return Some(());
                }

                if inside_url && import_data.url.is_some() {
                    (self.handle_warning)(Warning::DuplicateUrl {
                        range: Range::new(import_data.start, end),
                    });
                    return Some(());
                }

                let value = lexer.slice(start + 1, end - 1)?;
                import_data.url = Some(value);
                // For url("inside_url") url_range will determined in right_parenthesis
                if !inside_url {
                    import_data.url_range = Some(Range::new(start, end));
                }
            }
            Scope::InBlock => {
                let Some(last) = self.balanced.last() else {
                    return Some(());
                };
                let kind = match last.kind {
                    BalancedItemKind::Url => UrlRangeKind::String,
                    BalancedItemKind::ImageSet => UrlRangeKind::Function,
                    _ => return Some(()),
                };
                let value = lexer.slice(start + 1, end - 1)?;
                (self.handle_dependency)(Dependency::Url {
                    request: value,
                    range: Range::new(start, end),
                    kind,
                });
            }
            _ => {}
        }
        Some(())
    }

    fn at_keyword(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if name == "@namespace" {
            self.scope = Scope::AtNamespaceInvalid;
            (self.handle_warning)(Warning::NamespaceNotSupportedInBundledCss {
                range: Range::new(start, end),
            });
        } else if name == "@import" {
            if !self.allow_import_at_rule {
                self.scope = Scope::AtImportInvalid;
                (self.handle_warning)(Warning::NotPrecededAtImport {
                    range: Range::new(start, end),
                });
                return Some(());
            }
            self.scope = Scope::InAtImport(ImportData::new(start));
        } else if name == "@media"
            || name == "@supports"
            || name == "@layer"
            || name == "@container"
        {
            self.is_next_rule_prelude = true;
        }
        // else if self.allow_mode_switch {
        //     self.is_next_rule_prelude = false;
        // }
        Some(())
    }

    fn semicolon(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        match self.scope {
            Scope::InAtImport(ref import_data) => {
                let Some(url) = import_data.url else {
                    (self.handle_warning)(Warning::ExpectedUrl {
                        range: Range::new(import_data.start, end),
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                let Some(url_range) = &import_data.url_range else {
                    (self.handle_warning)(Warning::Unexpected {
                        unexpected: Range::new(start, end),
                        range: Range::new(import_data.start, end),
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                let layer = match &import_data.layer {
                    ImportDataLayer::None => None,
                    ImportDataLayer::EndLayer { value, range } => {
                        if url_range.start > range.start {
                            (self.handle_warning)(Warning::ExpectedBefore {
                                should_after: range.clone(),
                                range: url_range.clone(),
                            });
                            self.scope = Scope::TopLevel;
                            return Some(());
                        }
                        Some(*value)
                    }
                };
                let supports = match &import_data.supports {
                    ImportDataSupports::None => None,
                    ImportDataSupports::InSupports {
                        start: supports_start,
                    } => {
                        (self.handle_warning)(Warning::Unexpected {
                            unexpected: Range::new(start, end),
                            range: Range::new(*supports_start, end),
                        });
                        None
                    }
                    ImportDataSupports::EndSupports { value, range } => {
                        if url_range.start > range.start {
                            (self.handle_warning)(Warning::ExpectedBefore {
                                should_after: range.clone(),
                                range: url_range.clone(),
                            });
                            self.scope = Scope::TopLevel;
                            return Some(());
                        }
                        Some(*value)
                    }
                };
                if let Some(layer_range) = import_data.layer_range() {
                    if let Some(supports_range) = import_data.supports_range() {
                        if layer_range.start > supports_range.start {
                            (self.handle_warning)(Warning::ExpectedBefore {
                                should_after: supports_range.clone(),
                                range: layer_range.clone(),
                            });
                            self.scope = Scope::TopLevel;
                            return Some(());
                        }
                    }
                }
                let last_end = import_data
                    .supports_range()
                    .or_else(|| import_data.layer_range())
                    .unwrap_or(url_range)
                    .end;
                let media = self.get_media(lexer, last_end, start);
                (self.handle_dependency)(Dependency::Import {
                    request: url,
                    range: Range::new(import_data.start, end),
                    layer,
                    supports,
                    media,
                });
                self.scope = Scope::TopLevel;
            }
            Scope::AtImportInvalid | Scope::AtNamespaceInvalid => {
                self.scope = Scope::TopLevel;
            }
            Scope::InBlock => {
                // TODO: css modules
            }
            _ => {}
        }
        Some(())
    }

    fn function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        let name = lexer.slice(start, end - 1)?.to_ascii_lowercase();
        self.balanced.push(BalancedItem::new(&name, start, end));

        if let Scope::InAtImport(ref mut import_data) = self.scope {
            if name == "supports" {
                import_data.supports = ImportDataSupports::InSupports { start };
            }
        }
        Some(())
    }

    fn left_parenthesis(&mut self, _: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.balanced.push(BalancedItem::new_other(start, end));
        Some(())
    }

    fn right_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        let Some(last) = self.balanced.pop() else {
            return Some(());
        };
        if let Some(mode_data) = &mut self.mode_data {
            if matches!(
                last.kind,
                BalancedItemKind::Local | BalancedItemKind::Global
            ) {
                match self.balanced.last() {
                    Some(last) if matches!(last.kind, BalancedItemKind::Local) => {
                        mode_data.set_local()
                    }
                    Some(last) if matches!(last.kind, BalancedItemKind::Global) => {
                        mode_data.set_global()
                    }
                    _ => mode_data.set_none(),
                };
                (self.handle_dependency)(Dependency::Replace {
                    content: "",
                    range: Range::new(start, end),
                });
                return Some(());
            }
        }
        if let Scope::InAtImport(ref mut import_data) = self.scope {
            let not_in_supports = !import_data.in_supports();
            if matches!(last.kind, BalancedItemKind::Url) && not_in_supports {
                import_data.url_range = Some(Range::new(last.range.start, end));
            } else if matches!(last.kind, BalancedItemKind::Layer) && not_in_supports {
                import_data.layer = ImportDataLayer::EndLayer {
                    value: lexer.slice(last.range.end, end - 1)?,
                    range: Range::new(last.range.start, end),
                };
            } else if matches!(last.kind, BalancedItemKind::Supports) {
                import_data.supports = ImportDataSupports::EndSupports {
                    value: lexer.slice(last.range.end, end - 1)?,
                    range: Range::new(last.range.start, end),
                }
            }
        }
        Some(())
    }

    fn ident(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        match self.scope {
            Scope::InBlock => {
                // TODO: css modules
            }
            Scope::InAtImport(ref mut import_data) => {
                if lexer.slice(start, end)?.to_ascii_lowercase() == "layer" {
                    import_data.layer = ImportDataLayer::EndLayer {
                        value: "",
                        range: Range::new(start, end),
                    }
                }
            }
            _ => {}
        }
        Some(())
    }

    fn class(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_local_mode() {
            let start = start + 1;
            let name = lexer.slice(start, end)?;
            (self.handle_dependency)(Dependency::Local {
                name,
                range: Range::new(start, end),
                kind: LocalKind::Ident,
            })
        }
        Some(())
    }

    fn id(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_local_mode() {
            let start = start + 1;
            let name = lexer.slice(start, end)?;
            (self.handle_dependency)(Dependency::Local {
                name,
                range: Range::new(start, end),
                kind: LocalKind::Ident,
            })
        }
        Some(())
    }

    fn left_curly_bracket(&mut self, lexer: &mut Lexer, _: usize, _: usize) -> Option<()> {
        match self.scope {
            Scope::TopLevel => {
                self.allow_import_at_rule = false;
                self.scope = Scope::InBlock;
                self.block_nesting_level = 1;
                // if self.allow_mode_switch {
                //     self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
                // }
            }
            Scope::InBlock => {
                self.block_nesting_level += 1;
                // if self.allow_mode_switch {
                //     self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
                // }
            }
            _ => {}
        }
        Some(())
    }

    fn right_curly_bracket(&mut self, lexer: &mut Lexer, _: usize, _: usize) -> Option<()> {
        if matches!(self.scope, Scope::InBlock) {
            self.block_nesting_level -= 1;
            if self.block_nesting_level == 0 {
                // TODO: if isLocalMode
                self.scope = Scope::TopLevel;
                // if self.allow_mode_switch {
                //     self.is_next_rule_prelude = true;
                // }
            }
            // else if self.allow_mode_switch {
            //     self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
            // }
        }
        Some(())
    }

    fn pseudo_function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        let name = lexer.slice(start, end - 1)?.to_ascii_lowercase();
        self.balanced.push(BalancedItem::new(&name, start, end));
        if let Some(mode_data) = &mut self.mode_data {
            if name == ":global" {
                mode_data.set_global();
                (self.handle_dependency)(Dependency::Replace {
                    content: "",
                    range: Range::new(start, end),
                });
            } else if name == ":local" {
                mode_data.set_local();
                (self.handle_dependency)(Dependency::Replace {
                    content: "",
                    range: Range::new(start, end),
                });
            }
        }
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut Lexer<'s>, start: usize, end: usize) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if name == ":global" || name == ":local" {
            lexer.eat_white_space_and_comments()?;
            let end2 = lexer.cur_pos()?;
            let comments = lexer.slice(end, end2)?.trim_matches(is_white_space);
            (self.handle_dependency)(Dependency::Replace {
                content: comments,
                range: Range::new(start, end2),
            });
            if name == ":global" {
                mode_data.set_global();
            } else {
                mode_data.set_local();
            }
            return Some(());
        }
        if matches!(self.scope, Scope::TopLevel) && name == ":export" {
            self.lex_export(lexer, start);
        }
        Some(())
    }

    fn comma(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        if let Some(mode_data) = &mut self.mode_data {
            mode_data.set_none();
        }
        Some(())
    }
}
