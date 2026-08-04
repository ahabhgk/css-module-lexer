use crate::Range;
use std::str;

pub const C_LINE_FEED: u8 = b'\n';
pub const C_CARRIAGE_RETURN: u8 = b'\r';
pub const C_FORM_FEED: u8 = b'\x0c';

pub const C_TAB: u8 = b'\t';
pub const C_SPACE: u8 = b' ';

pub const C_SOLIDUS: u8 = b'/';
pub const C_REVERSE_SOLIDUS: u8 = b'\\';
pub const C_ASTERISK: u8 = b'*';

pub const C_LEFT_PARENTHESIS: u8 = b'(';
pub const C_RIGHT_PARENTHESIS: u8 = b')';
pub const C_LEFT_CURLY: u8 = b'{';
pub const C_RIGHT_CURLY: u8 = b'}';
pub const C_LEFT_SQUARE: u8 = b'[';
pub const C_RIGHT_SQUARE: u8 = b']';

pub const C_QUOTATION_MARK: u8 = b'"';
pub const C_APOSTROPHE: u8 = b'\'';

pub const C_FULL_STOP: u8 = b'.';
pub const C_COLON: u8 = b':';
pub const C_SEMICOLON: u8 = b';';
pub const C_COMMA: u8 = b',';
pub const C_PERCENTAGE: u8 = b'%';
pub const C_AT_SIGN: u8 = b'@';

pub const C_LOW_LINE: u8 = b'_';
pub const C_LOWER_E: u8 = b'e';
pub const C_UPPER_E: u8 = b'E';

pub const C_NUMBER_SIGN: u8 = b'#';
pub const C_PLUS_SIGN: u8 = b'+';
pub const C_HYPHEN_MINUS: u8 = b'-';

pub const C_LESS_THAN_SIGN: u8 = b'<';
pub const C_GREATER_THAN_SIGN: u8 = b'>';

pub type Pos = u32;

/// The lexical kind of a CSS token.
///
/// Tokens retain only byte ranges into the original source.  Comments are
/// emitted deliberately so that CSS Modules magic comments can be interpreted
/// without a second scan of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Eof,
    Ident,
    AtKeyword,
    Hash,
    IdHash,
    QuotedString,
    Url,
    Function,
    Number,
    Percentage,
    Dimension,
    WhiteSpace,
    Comment,
    BadComment,
    BadString,
    BadUrl,
    Delim,
    Colon,
    Semicolon,
    Comma,
    LeftParenthesis,
    RightParenthesis,
    LeftSquareBracket,
    RightSquareBracket,
    LeftCurlyBracket,
    RightCurlyBracket,
    IncludeMatch,
    DashMatch,
    PrefixMatch,
    SuffixMatch,
    SubstringMatch,
}

impl TokenKind {
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(self, Self::WhiteSpace | Self::Comment)
    }

    #[inline]
    pub fn is_scan_error(self) -> bool {
        matches!(self, Self::BadComment | Self::BadString | Self::BadUrl)
    }
}

/// A CSS token represented by ranges into the input string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: Range,
    pub value_range: Range,
}

impl Token {
    #[inline]
    pub const fn new(kind: TokenKind, range: Range, value_range: Range) -> Self {
        Self {
            kind,
            range,
            value_range,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trivia {
    pub range: Range,
    pub end: Pos,
    pub first_comment_start: Option<Pos>,
    pub has_white_space: bool,
}

impl Trivia {
    #[inline]
    pub fn has_whitespace(self) -> bool {
        self.has_white_space
    }
}

/// A significant token together with all immediately preceding whitespace and
/// comments.  The leading trivia is consumed once and never rescanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenWithTrivia {
    pub token: Token,
    pub leading: Trivia,
}

pub trait Visitor<'s> {
    fn comment(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }
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
    value: &'s [u8],
    cur_pos: Option<Pos>,
    token_pos: Pos,
}

impl<'s> Lexer<'s> {
    pub fn new(value: &'s str) -> Self {
        assert!(value.len() <= Pos::MAX as usize, "CSS input is too large");
        Self {
            value: value.as_bytes(),
            cur_pos: None,
            token_pos: 0,
        }
    }

    #[inline]
    pub(crate) fn reset_token_stream(&mut self) {
        self.cur_pos = None;
        self.token_pos = 0;
    }

    #[inline]
    pub(crate) fn set_legacy_position(&mut self, position: Pos) {
        self.cur_pos = Some(position);
    }

    #[inline]
    pub(crate) fn source_end(&self) -> Pos {
        self.value.len() as Pos
    }

    #[inline]
    pub(crate) fn sync_token_stream(&mut self, fallback: Pos) {
        self.token_pos = self.cur_pos.unwrap_or(fallback);
    }

    pub(crate) fn token_cursor_at(&self, position: Pos) -> TokenCursor<'s> {
        let mut lexer = self.clone();
        lexer.cur_pos = None;
        lexer.token_pos = position;
        TokenCursor::from_lexer(lexer)
    }

    pub(crate) fn token_lexer_at(&self, position: Pos) -> Lexer<'s> {
        let mut lexer = self.clone();
        lexer.cur_pos = None;
        lexer.token_pos = position;
        lexer
    }

    /// Classify the next non-trivia item without scanning the complete token.
    ///
    /// Nested-rule detection only needs to distinguish an ident/function
    /// prelude from selector punctuation. Keeping this byte-level fast path
    /// avoids tokenizing the same property name once for lookahead and again
    /// when the main stream reaches it.
    pub(crate) fn next_significant_is_not_ident(&self, position: Pos) -> Option<bool> {
        let bytes = self.value;
        let mut position = position as usize;
        loop {
            while position < bytes.len() && is_white_space(bytes[position]) {
                position += 1;
            }
            if position + 1 < bytes.len()
                && bytes[position] == C_SOLIDUS
                && bytes[position + 1] == C_ASTERISK
            {
                let (kind, end, _, _) = self.scan_comment(position);
                if kind == TokenKind::BadComment {
                    return Some(true);
                }
                position = end;
                continue;
            }
            let byte = *bytes.get(position)?;
            return Some(!self.starts_ident_at(position) && byte != C_RIGHT_CURLY);
        }
    }

    pub(crate) fn slice(&self, start: Pos, end: Pos) -> Option<&'s str> {
        let range = Range::new(start, end);
        let start = start as usize;
        let end = end as usize;
        if start > end || end > self.value.len() {
            return None;
        }
        // SAFETY: Lexer-generated positions always delimit complete tokens or
        // source fragments and therefore stay on UTF-8 code point boundaries.
        Some(unsafe { Self::slice_unchecked(self.value, &range) })
    }

    pub fn slice_range<'a>(input: &'a str, range: &Range) -> Option<&'a str> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end
            || end > input.len()
            || !input.is_char_boundary(start)
            || !input.is_char_boundary(end)
        {
            return None;
        }
        // SAFETY: The range was checked against the original `str` above.
        Some(unsafe { Self::slice_unchecked(input.as_bytes(), range) })
    }

    unsafe fn slice_unchecked<'a>(input: &'a [u8], range: &Range) -> &'a str {
        let value = input.get_unchecked(range.start as usize..range.end as usize);
        str::from_utf8_unchecked(value)
    }
}

impl Lexer<'_> {
    #[inline]
    fn byte_at(&self, pos: Pos) -> Option<u8> {
        let pos = pos as usize;
        self.value.get(pos).copied()
    }

    #[inline]
    pub fn consume(&mut self) {
        if self.cur_pos.is_none() {
            self.cur_pos = Some(0);
        } else if self.cur().is_some() {
            self.cur_pos = self.cur_pos.map(|pos| pos + 1);
        }
    }

    #[inline]
    pub fn cur_pos(&self) -> Option<Pos> {
        self.cur_pos
    }

    #[inline]
    pub fn cur(&self) -> Option<u8> {
        self.cur_pos.and_then(|pos| self.byte_at(pos))
    }

    #[inline]
    pub fn peek_pos(&self) -> Option<Pos> {
        match self.cur_pos {
            Some(pos) if self.cur().is_some() => Some(pos + 1),
            Some(_) => None,
            None => Some(0),
        }
    }

    #[inline]
    pub fn peek(&self) -> Option<u8> {
        self.peek_pos().and_then(|pos| self.byte_at(pos))
    }

    #[inline]
    pub fn peek2_pos(&self) -> Option<Pos> {
        self.peek_pos().and_then(|pos| self.peek().map(|_| pos + 1))
    }

    #[inline]
    pub fn peek2(&self) -> Option<u8> {
        self.peek2_pos().and_then(|pos| self.byte_at(pos))
    }
}

impl<'s> Lexer<'s> {
    /// Return the next CSS token without allocating or decoding its value.
    ///
    /// `TokenKind::Eof` is returned after the input is exhausted. Unterminated
    /// comments, strings, and URLs are represented by the corresponding
    /// `Bad*` token kinds, so EOF and tokenizer errors are never conflated.
    pub fn next_token(&mut self) -> Token {
        let start = self.token_pos as usize;
        let len = self.value.len();
        if start >= len {
            self.token_pos = len as Pos;
            return Token::new(
                TokenKind::Eof,
                Range::new(len as Pos, len as Pos),
                Range::new(len as Pos, len as Pos),
            );
        }

        let byte = self.value[start];
        let (kind, end, value_start, value_end) = match byte {
            C_SPACE | C_TAB | C_LINE_FEED | C_CARRIAGE_RETURN | C_FORM_FEED => {
                self.scan_whitespace(start)
            }
            C_QUOTATION_MARK => self.scan_string(start, C_QUOTATION_MARK),
            C_APOSTROPHE => self.scan_string(start, C_APOSTROPHE),
            C_NUMBER_SIGN => {
                let value_start = start + 1;
                if self.starts_ident_at(value_start) {
                    let end = self.scan_name(value_start);
                    (TokenKind::IdHash, end, value_start, end)
                } else if self
                    .value
                    .get(value_start)
                    .is_some_and(|byte| is_digit(*byte) || *byte == C_HYPHEN_MINUS)
                {
                    let end = self.scan_name(value_start);
                    (TokenKind::Hash, end, value_start, end)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_LEFT_PARENTHESIS => (TokenKind::LeftParenthesis, start + 1, start, start + 1),
            C_RIGHT_PARENTHESIS => (TokenKind::RightParenthesis, start + 1, start, start + 1),
            C_LEFT_SQUARE => (TokenKind::LeftSquareBracket, start + 1, start, start + 1),
            C_RIGHT_SQUARE => (TokenKind::RightSquareBracket, start + 1, start, start + 1),
            C_LEFT_CURLY => (TokenKind::LeftCurlyBracket, start + 1, start, start + 1),
            C_RIGHT_CURLY => (TokenKind::RightCurlyBracket, start + 1, start, start + 1),
            C_COLON => (TokenKind::Colon, start + 1, start, start + 1),
            C_SEMICOLON => (TokenKind::Semicolon, start + 1, start, start + 1),
            C_COMMA => (TokenKind::Comma, start + 1, start, start + 1),
            C_PLUS_SIGN => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_HYPHEN_MINUS => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else if self.starts_ident_at(start) {
                    self.scan_ident_like(start)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_FULL_STOP => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_SOLIDUS => {
                if self.value.get(start + 1) == Some(&C_ASTERISK) {
                    self.scan_comment(start)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_AT_SIGN => {
                let value_start = start + 1;
                if self.starts_ident_at(value_start) {
                    let end = self.scan_name(value_start);
                    (TokenKind::AtKeyword, end, value_start, end)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            C_REVERSE_SOLIDUS => {
                if self.is_valid_escape_at(start) {
                    self.scan_ident_like(start)
                } else {
                    (TokenKind::Delim, start + 1, start, start + 1)
                }
            }
            b'0'..=b'9' => self.scan_numeric(start),
            b'$' if self.value[start..].starts_with(b"$=") => {
                (TokenKind::SuffixMatch, start + 2, start, start + 2)
            }
            b'^' if self.value[start..].starts_with(b"^=") => {
                (TokenKind::PrefixMatch, start + 2, start, start + 2)
            }
            b'|' if self.value[start..].starts_with(b"|=") => {
                (TokenKind::DashMatch, start + 2, start, start + 2)
            }
            b'~' if self.value[start..].starts_with(b"~=") => {
                (TokenKind::IncludeMatch, start + 2, start, start + 2)
            }
            b'*' if self.value[start..].starts_with(b"*=") => {
                (TokenKind::SubstringMatch, start + 2, start, start + 2)
            }
            _ if self.starts_ident_at(start) => self.scan_ident_like(start),
            _ => (TokenKind::Delim, start + 1, start, start + 1),
        };

        self.token_pos = end as Pos;
        Token::new(
            kind,
            Range::new(start as Pos, end as Pos),
            Range::new(value_start as Pos, value_end as Pos),
        )
    }

    /// Drive the legacy Visitor API from the streaming tokenizer.
    ///
    /// The adapter keeps the old callback surface temporarily, but token
    /// boundaries, comments, strings, and URLs are obtained from the same
    /// forward-only scanner exposed by [`Lexer::next_token`].  Selector
    /// punctuation is kept as a one-token lookahead so declarations and
    /// nested rules can still be distinguished by the existing dependency
    /// state machine.
    pub fn lex_tokens<T: Visitor<'s>>(&mut self, visitor: &mut T) {
        self.token_pos = 0;
        self.cur_pos = None;

        loop {
            let token = self.next_token();
            if token.kind == TokenKind::Eof {
                self.cur_pos = Some(token.range.start);
                return;
            }

            let mut result = Some(());
            match token.kind {
                TokenKind::Comment | TokenKind::BadComment => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.comment(self, token.range.start, token.range.end);
                }
                TokenKind::WhiteSpace => {
                    self.cur_pos = Some(token.range.end);
                }
                TokenKind::QuotedString | TokenKind::BadString => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.string(self, token.range.start, token.range.end);
                }
                TokenKind::Url => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.url(
                        self,
                        token.range.start,
                        token.range.end,
                        token.value_range.start,
                        token.value_range.end,
                    );
                }
                TokenKind::Function => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.function(self, token.range.start, token.range.end);
                }
                TokenKind::Ident => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.ident(self, token.range.start, token.range.end);
                }
                TokenKind::AtKeyword => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.at_keyword(self, token.range.start, token.range.end);
                }
                TokenKind::IdHash | TokenKind::Hash => {
                    let hash_end = token.range.start + 1;
                    self.cur_pos = Some(hash_end);
                    if visitor.is_selector(self).unwrap_or(false) {
                        if token.kind == TokenKind::Hash && !self.starts_ident_at(hash_end as usize)
                        {
                            self.token_pos = hash_end;
                            result = visitor.id(self, token.range.start, hash_end);
                        } else {
                            self.token_pos = token.range.end;
                            self.cur_pos = Some(token.range.end);
                            result = visitor.id(self, token.range.start, token.range.end);
                        }
                    } else {
                        self.token_pos = hash_end;
                    }
                }
                TokenKind::Delim if self.slice(token.range.start, token.range.end) == Some("#") => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.id(self, token.range.start, token.range.end);
                }
                TokenKind::Delim if self.slice(token.range.start, token.range.end) == Some(".") => {
                    let mut lookahead = self.clone();
                    let next = lookahead.next_token();
                    let class_end =
                        if next.kind == TokenKind::Ident && next.range.start == token.range.end {
                            next.range.end
                        } else {
                            token.range.end
                        };
                    self.cur_pos = Some(token.range.end);
                    if visitor.is_selector(self).unwrap_or(false) {
                        self.token_pos = class_end;
                        self.cur_pos = Some(class_end);
                        result = visitor.class(self, token.range.start, class_end);
                    } else {
                        self.token_pos = token.range.end;
                    }
                }
                TokenKind::Colon => {
                    let mut lookahead = self.clone();
                    let next = lookahead.next_token();
                    if !matches!(next.kind, TokenKind::Ident | TokenKind::Function)
                        || next.range.start != token.range.end
                    {
                        self.cur_pos = Some(token.range.end);
                        continue;
                    }
                    let (end, function) = if next.kind == TokenKind::Function {
                        (next.range.end, true)
                    } else {
                        let after_name = lookahead.next_token();
                        if after_name.kind == TokenKind::LeftParenthesis
                            && after_name.range.start == next.range.end
                        {
                            (after_name.range.end, true)
                        } else {
                            (next.range.end, false)
                        }
                    };
                    self.cur_pos = Some(token.range.end);
                    if !visitor.is_selector(self).unwrap_or(false) {
                        self.token_pos = token.range.end;
                        continue;
                    }
                    self.token_pos = end;
                    self.cur_pos = Some(end);
                    result = if function {
                        visitor.pseudo_function(self, token.range.start, end)
                    } else {
                        visitor.pseudo_class(self, token.range.start, end)
                    };
                }
                TokenKind::LeftParenthesis => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.left_parenthesis(self, token.range.start, token.range.end);
                }
                TokenKind::RightParenthesis => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.right_parenthesis(self, token.range.start, token.range.end);
                }
                TokenKind::Comma => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.comma(self, token.range.start, token.range.end);
                }
                TokenKind::Semicolon => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.semicolon(self, token.range.start, token.range.end);
                }
                TokenKind::LeftCurlyBracket => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.left_curly_bracket(self, token.range.start, token.range.end);
                }
                TokenKind::RightCurlyBracket => {
                    self.cur_pos = Some(token.range.end);
                    result = visitor.right_curly_bracket(self, token.range.start, token.range.end);
                }
                _ => {
                    self.cur_pos = Some(token.range.end);
                }
            }

            if result.is_none() {
                return;
            }
            self.token_pos = self.cur_pos.unwrap_or(token.range.end);
        }
    }

    #[inline]
    fn scan_whitespace(&self, start: usize) -> (TokenKind, usize, usize, usize) {
        let bytes = self.value;
        let mut end = start;
        while end < bytes.len() && is_white_space(bytes[end]) {
            end += 1;
        }
        (TokenKind::WhiteSpace, end, start, end)
    }

    fn scan_comment(&self, start: usize) -> (TokenKind, usize, usize, usize) {
        let bytes = self.value;
        let mut end = start + 2;
        while end + 1 < bytes.len() {
            if bytes[end] == C_ASTERISK && bytes[end + 1] == C_SOLIDUS {
                return (TokenKind::Comment, end + 2, start + 2, end);
            }
            end += if bytes[end] < 0x80 {
                1
            } else {
                self.utf8_width_at(end)
            };
        }
        (TokenKind::BadComment, bytes.len(), start + 2, bytes.len())
    }

    fn scan_string(&self, start: usize, quote: u8) -> (TokenKind, usize, usize, usize) {
        let mut end = start + 1;
        while end < self.value.len() {
            let byte = self.value[end];
            if byte == quote {
                return (TokenKind::QuotedString, end + 1, start + 1, end);
            }
            if is_new_line(byte) {
                return (TokenKind::BadString, end, start + 1, end);
            }
            if byte == C_REVERSE_SOLIDUS {
                if self
                    .value
                    .get(end + 1)
                    .is_some_and(|next| is_new_line(*next))
                {
                    end += 1;
                    if self.value.get(end) == Some(&C_CARRIAGE_RETURN)
                        && self.value.get(end + 1) == Some(&C_LINE_FEED)
                    {
                        end += 1;
                    }
                    end += 1;
                } else if self.is_valid_escape_at(end) {
                    end = self.scan_escape(end);
                } else {
                    end += 1;
                }
            } else {
                end += self.utf8_width_at(end);
            }
        }
        (TokenKind::BadString, end, start + 1, end)
    }

    fn scan_ident_like(&self, start: usize) -> (TokenKind, usize, usize, usize) {
        let name_end = self.scan_name(start);
        if self.value.get(name_end) != Some(&C_LEFT_PARENTHESIS) {
            return (TokenKind::Ident, name_end, start, name_end);
        }

        let open_end = name_end + 1;
        let name = &self.value[start..name_end];
        let is_url = name.eq_ignore_ascii_case(b"url")
            || (name
                .iter()
                .any(|byte| matches!(*byte, C_REVERSE_SOLIDUS | 0))
                && crate::escape::eq_ignore_ascii_case(
                    std::str::from_utf8(name).unwrap_or_default(),
                    "url",
                ));
        if is_url {
            let content_start = self.skip_white_space(open_end);
            if !matches!(
                self.value.get(content_start),
                Some(&C_QUOTATION_MARK) | Some(&C_APOSTROPHE)
            ) {
                return self.scan_url(start, content_start);
            }
        }
        (TokenKind::Function, open_end, start, name_end)
    }

    fn scan_url(&self, start: usize, content_start: usize) -> (TokenKind, usize, usize, usize) {
        let mut end = content_start;
        while end < self.value.len() {
            let byte = self.value[end];
            if byte == C_RIGHT_PARENTHESIS {
                return (TokenKind::Url, end + 1, content_start, end);
            }
            if is_white_space(byte) {
                let content_end = end;
                let close = self.skip_white_space(end);
                if self.value.get(close) == Some(&C_RIGHT_PARENTHESIS) {
                    return (TokenKind::Url, close + 1, content_start, content_end);
                }
                return self.scan_bad_url(start, close, content_start);
            }
            if byte == C_QUOTATION_MARK
                || byte == C_APOSTROPHE
                || byte == C_LEFT_PARENTHESIS
                || is_non_printable(byte)
            {
                return self.scan_bad_url(start, end, content_start);
            }
            if byte == C_REVERSE_SOLIDUS {
                if self.is_valid_escape_at(end) {
                    end = self.scan_escape(end);
                } else if self
                    .value
                    .get(end + 1)
                    .is_some_and(|next| is_new_line(*next))
                {
                    end += 2;
                    if self.value.get(end - 1) == Some(&C_CARRIAGE_RETURN)
                        && self.value.get(end) == Some(&C_LINE_FEED)
                    {
                        end += 1;
                    }
                } else {
                    return self.scan_bad_url(start, end, content_start);
                }
            } else {
                end += self.utf8_width_at(end);
            }
        }
        (
            TokenKind::BadUrl,
            self.value.len(),
            content_start,
            self.value.len(),
        )
    }

    fn scan_bad_url(
        &self,
        _start: usize,
        mut end: usize,
        content_start: usize,
    ) -> (TokenKind, usize, usize, usize) {
        while end < self.value.len() {
            if self.value[end] == C_RIGHT_PARENTHESIS {
                end += 1;
                break;
            }
            if self.value[end] == C_REVERSE_SOLIDUS {
                if self.is_valid_escape_at(end) {
                    end = self.scan_escape(end);
                } else if self
                    .value
                    .get(end + 1)
                    .is_some_and(|next| is_new_line(*next))
                {
                    end += 2;
                    if self.value.get(end - 1) == Some(&C_CARRIAGE_RETURN)
                        && self.value.get(end) == Some(&C_LINE_FEED)
                    {
                        end += 1;
                    }
                } else {
                    end += 1;
                }
            } else {
                end += self.utf8_width_at(end);
            }
        }
        (TokenKind::BadUrl, end, content_start, end)
    }

    fn scan_numeric(&self, start: usize) -> (TokenKind, usize, usize, usize) {
        let mut end = start;
        if matches!(
            self.value.get(end),
            Some(&C_PLUS_SIGN) | Some(&C_HYPHEN_MINUS)
        ) {
            end += 1;
        }
        while self.value.get(end).is_some_and(|byte| is_digit(*byte)) {
            end += 1;
        }
        if self.value.get(end) == Some(&C_FULL_STOP)
            && self.value.get(end + 1).is_some_and(|byte| is_digit(*byte))
        {
            end += 1;
            while self.value.get(end).is_some_and(|byte| is_digit(*byte)) {
                end += 1;
            }
        }
        if matches!(self.value.get(end), Some(&C_LOWER_E) | Some(&C_UPPER_E)) {
            let exponent_start = end;
            let mut exponent_end = end + 1;
            if matches!(
                self.value.get(exponent_end),
                Some(&C_PLUS_SIGN) | Some(&C_HYPHEN_MINUS)
            ) {
                exponent_end += 1;
            }
            if self
                .value
                .get(exponent_end)
                .is_some_and(|byte| is_digit(*byte))
            {
                end = exponent_end + 1;
                while self.value.get(end).is_some_and(|byte| is_digit(*byte)) {
                    end += 1;
                }
            } else {
                end = exponent_start;
            }
        }
        if self.value.get(end) == Some(&C_PERCENTAGE) {
            return (TokenKind::Percentage, end + 1, start, end + 1);
        }
        if self.starts_ident_at(end) {
            let end = self.scan_name(end);
            return (TokenKind::Dimension, end, start, end);
        }
        (TokenKind::Number, end, start, end)
    }

    #[inline]
    fn skip_white_space(&self, mut position: usize) -> usize {
        while self
            .value
            .get(position)
            .is_some_and(|byte| is_white_space(*byte))
        {
            position += 1;
        }
        position
    }

    #[inline]
    fn scan_name(&self, mut position: usize) -> usize {
        let bytes = self.value;
        while position < bytes.len() {
            let byte = bytes[position];
            if byte == C_REVERSE_SOLIDUS {
                if self.is_valid_escape_at(position) {
                    position = self.scan_escape(position);
                } else {
                    break;
                }
            } else if is_ident(byte) || byte == 0 {
                position += if byte < 0x80 {
                    1
                } else {
                    self.utf8_width_at(position)
                };
            } else {
                break;
            }
        }
        position
    }

    #[inline]
    fn starts_ident_at(&self, position: usize) -> bool {
        let Some(byte) = self.value.get(position).copied() else {
            return false;
        };
        match byte {
            C_HYPHEN_MINUS => match self.value.get(position + 1).copied() {
                Some(next) => {
                    is_name_start_byte(next)
                        || next == C_HYPHEN_MINUS
                        || self.is_valid_escape_at(position + 1)
                }
                None => false,
            },
            C_REVERSE_SOLIDUS => self.is_valid_escape_at(position),
            _ => is_name_start_byte(byte),
        }
    }

    #[inline]
    fn starts_number_at(&self, position: usize) -> bool {
        let Some(first) = self.value.get(position).copied() else {
            return false;
        };
        let second = self.value.get(position + 1).copied();
        let third = self.value.get(position + 2).copied();
        match first {
            C_PLUS_SIGN | C_HYPHEN_MINUS => {
                second.is_some_and(is_digit)
                    || (second == Some(C_FULL_STOP) && third.is_some_and(is_digit))
            }
            C_FULL_STOP => second.is_some_and(is_digit),
            _ => is_digit(first),
        }
    }

    #[inline]
    fn is_valid_escape_at(&self, position: usize) -> bool {
        self.value.get(position) == Some(&C_REVERSE_SOLIDUS)
            && self
                .value
                .get(position + 1)
                .is_some_and(|byte| !is_new_line(*byte))
    }

    #[inline]
    fn scan_escape(&self, position: usize) -> usize {
        debug_assert!(self.is_valid_escape_at(position));
        let mut end = position + 1;
        if self.value[end].is_ascii_hexdigit() {
            let mut digits = 0;
            while end < self.value.len() && digits < 6 && self.value[end].is_ascii_hexdigit() {
                end += 1;
                digits += 1;
            }
            if self
                .value
                .get(end)
                .is_some_and(|byte| is_white_space(*byte))
            {
                end += 1;
                if self.value.get(end - 1) == Some(&C_CARRIAGE_RETURN)
                    && self.value.get(end) == Some(&C_LINE_FEED)
                {
                    end += 1;
                }
            }
            return end;
        }
        end + self.utf8_width_at(end)
    }

    #[inline]
    fn utf8_width_at(&self, position: usize) -> usize {
        let byte = self.value[position];
        if byte < 0x80 {
            1
        } else if byte < 0xE0 {
            2
        } else if byte < 0xF0 {
            3
        } else {
            4
        }
    }

    pub fn lex<T: Visitor<'s>>(&mut self, visitor: &mut T) {
        self.lex_impl(visitor);
    }

    fn lex_impl<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        while self.cur().is_some() {
            self.consume_comments_with_visitor(visitor)?;
            // https://drafts.csswg.org/css-syntax/#consume-token
            match self.cur()? {
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
                C_LEFT_SQUARE => self.consume_delim(),
                C_REVERSE_SOLIDUS => self.consume_reverse_solidus(visitor)?,
                C_RIGHT_SQUARE => self.consume_delim(),
                C_LEFT_CURLY => self.consume_left_curly(visitor)?,
                C_RIGHT_CURLY => self.consume_right_curly(visitor)?,
                c if is_digit(c) => self.consume_numeric_token()?,
                c if is_ident_start(c) => self.consume_ident_like(visitor)?,
                _ => self.consume_delim(),
            }
        }
        Some(())
    }

    pub fn consume_comments_with_visitor<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        if self.cur()? == C_SOLIDUS && self.peek()? == C_ASTERISK {
            let start = self.cur_pos()?;
            self.consume();
            loop {
                self.consume();
                let c = self.cur()?;
                if c == C_ASTERISK && self.peek()? == C_SOLIDUS {
                    self.consume();
                    self.consume();
                    break;
                }
            }
            let end = self.cur_pos()?;
            visitor.comment(self, start, end)?;
        }
        Some(())
    }

    pub fn consume_white_space_and_comments_with_visitor<T: Visitor<'s>>(
        &mut self,
        visitor: &mut T,
    ) -> Option<()> {
        loop {
            self.consume_comments_with_visitor(visitor)?;
            if is_white_space(self.cur()?) {
                self.consume_space()?;
            } else {
                break;
            }
        }
        Some(())
    }

    pub fn consume_delim(&mut self) {
        self.consume();
    }

    pub fn consume_numeric_token(&mut self) -> Option<()> {
        self.consume_number()?;
        let c = self.cur()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            return self.consume_ident_sequence();
        }
        if c == C_PERCENTAGE {
            self.consume();
        }
        Some(())
    }

    pub fn consume_number(&mut self) -> Option<()> {
        self.consume();
        while is_digit(self.cur()?) {
            self.consume();
        }
        if self.cur()? == C_FULL_STOP && is_digit(self.peek()?) {
            self.consume();
            self.consume();
            while is_digit(self.cur()?) {
                self.consume();
            }
        }
        let c = self.cur()?;
        if c == C_LOWER_E || c == C_UPPER_E {
            let c = self.peek()?;
            if is_digit(c) {
                self.consume();
            } else if c == C_HYPHEN_MINUS || c == C_PLUS_SIGN {
                let c = self.peek2()?;
                if is_digit(c) {
                    self.consume();
                    self.consume();
                } else {
                    return Some(());
                }
            } else {
                return Some(());
            }
        } else {
            return Some(());
        }
        self.consume();
        while is_digit(self.cur()?) {
            self.consume();
        }
        Some(())
    }

    pub fn consume_ident_sequence(&mut self) -> Option<()> {
        loop {
            let c = self.cur()?;
            if maybe_valid_escape(c) {
                self.consume();
                self.consume_escaped()?;
            } else if is_ident(c) {
                self.consume();
            } else {
                return Some(());
            }
        }
    }

    pub fn consume_escaped(&mut self) -> Option<()> {
        if self.cur().is_some_and(is_hex_digit) {
            let mut digits = 1;
            while digits < 6 {
                self.consume();
                if self.cur().is_some_and(is_hex_digit) {
                    digits += 1;
                } else {
                    break;
                }
            }
            if self.cur().is_some_and(is_white_space) {
                self.consume();
            }
        } else if self.cur().is_some() {
            self.consume();
        }
        Some(())
    }

    pub fn consume_ident_like<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume_ident_sequence()?;
        let peek_pos = self.peek_pos()?;
        if self.cur_pos()? == start + 3 && self.slice(start, peek_pos)?.eq_ignore_ascii_case("url(")
        {
            self.consume();
            while is_white_space(self.cur()?) {
                self.consume();
            }
            let c = self.cur()?;
            if c == C_QUOTATION_MARK || c == C_APOSTROPHE {
                visitor.function(self, start, peek_pos)
            } else {
                self.consume_url(visitor, start)
            }
        } else if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume();
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
                self.consume();
                self.consume_escaped()?;
            } else if is_white_space(c) {
                let content_end = self.cur_pos()?;
                self.consume();
                while is_white_space(self.cur()?) {
                    self.consume();
                }
                if self.cur()? != C_RIGHT_PARENTHESIS {
                    return Some(());
                }
                self.consume();
                return visitor.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_RIGHT_PARENTHESIS {
                let content_end = self.cur_pos()?;
                self.consume();
                return visitor.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_LEFT_PARENTHESIS {
                return Some(());
            } else {
                self.consume();
            }
        }
    }

    pub fn consume_string<T: Visitor<'s>>(&mut self, visitor: &mut T, end: u8) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume();
        loop {
            let c = self.cur()?;
            if c == end {
                self.consume();
                break;
            } else if is_new_line(c) {
                break;
            } else if c == C_REVERSE_SOLIDUS {
                self.consume();
                let c2 = self.cur()?;
                if is_new_line(c2) {
                    self.consume();
                } else if are_valid_escape(c, c2) {
                    self.consume_escaped()?;
                }
            } else {
                self.consume();
            }
        }
        visitor.string(self, start, self.cur_pos()?)
    }

    pub fn consume_number_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c2 = self.peek()?;
        let start = self.cur_pos()?;
        if is_ident(c2) || are_valid_escape(c2, self.peek2()?) {
            self.consume();
            if !visitor.is_selector(self)? {
                return Some(());
            }
            if !start_ident_sequence(self.cur()?, self.peek()?, self.peek2()?) {
                return visitor.id(self, start, self.cur_pos()?);
            }
            self.consume_ident_sequence()?;
            visitor.id(self, start, self.cur_pos()?)
        } else {
            self.consume_delim();
            visitor.id(self, start, self.cur_pos()?)
        }
    }

    pub fn consume_left_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.left_parenthesis(self, end - 1, end)
    }

    pub fn consume_right_parenthesis<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.right_parenthesis(self, end - 1, end)
    }

    pub fn consume_plus_sign(&mut self) -> Option<()> {
        if start_number(self.cur()?, self.peek()?, self.peek2()?) {
            self.consume_numeric_token()?;
        } else {
            self.consume_delim();
        }
        Some(())
    }

    pub fn consume_comma<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.comma(self, end - 1, end)
    }

    pub fn consume_minus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            self.consume_numeric_token()?;
        } else if c2 == C_HYPHEN_MINUS && c3 == C_GREATER_THAN_SIGN {
            self.consume();
            self.consume();
        } else if start_ident_sequence(c, c2, c3) {
            self.consume_ident_like(visitor)?;
        } else {
            self.consume_delim();
        }
        Some(())
    }

    pub fn consume_full_stop<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            return self.consume_numeric_token();
        }
        let start = self.cur_pos()?;
        self.consume();
        if !visitor.is_selector(self)? {
            return Some(());
        }
        if !start_ident_sequence(c2, c3, self.peek2()?) {
            return visitor.class(self, start, self.cur_pos()?);
        }
        self.consume_ident_sequence()?;
        visitor.class(self, start, self.cur_pos()?)
    }

    pub fn consume_potential_pseudo<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume();
        if !start_ident_sequence(self.cur()?, self.peek()?, self.peek2()?) {
            return Some(());
        }
        if !visitor.is_selector(self)? {
            return Some(());
        }
        self.consume_ident_sequence()?;
        if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume();
            visitor.pseudo_function(self, start, self.cur_pos()?)
        } else {
            visitor.pseudo_class(self, start, self.cur_pos()?)
        }
    }

    pub fn consume_semicolon<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.semicolon(self, end - 1, end)
    }

    pub fn consume_less_than_sign(&mut self) -> Option<()> {
        self.consume();
        if self.cur()? == b'!' && self.peek()? == b'-' && self.peek2()? == b'-' {
            self.consume();
            self.consume();
            self.consume();
        }
        Some(())
    }

    pub fn consume_at_sign<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume();
        if start_ident_sequence(self.cur()?, self.peek()?, self.peek2()?) {
            self.consume_ident_sequence()?;
            return visitor.at_keyword(self, start, self.cur_pos()?);
        }
        Some(())
    }

    pub fn consume_reverse_solidus<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        if are_valid_escape(self.cur()?, self.peek()?) {
            self.consume_ident_like(visitor)?;
        } else {
            self.consume_delim();
        }
        Some(())
    }

    pub fn consume_left_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.left_curly_bracket(self, end - 1, end)
    }

    pub fn consume_right_curly<T: Visitor<'s>>(&mut self, visitor: &mut T) -> Option<()> {
        self.consume();
        let end = self.cur_pos()?;
        visitor.right_curly_bracket(self, end - 1, end)
    }
}

/// A cursor that returns significant tokens with their leading trivia folded
/// into one small value.
pub struct TokenCursor<'s> {
    lexer: Lexer<'s>,
    peeked: Option<TokenWithTrivia>,
}

impl<'s> TokenCursor<'s> {
    pub fn new(input: &'s str) -> Self {
        Self {
            lexer: Lexer::new(input),
            peeked: None,
        }
    }

    pub fn from_lexer(lexer: Lexer<'s>) -> Self {
        Self {
            lexer,
            peeked: None,
        }
    }

    #[allow(clippy::should_implement_trait)]
    #[inline]
    pub fn next(&mut self) -> TokenWithTrivia {
        self.peeked
            .take()
            .unwrap_or_else(|| self.read_significant())
    }

    #[inline]
    pub fn peek(&mut self) -> TokenWithTrivia {
        if self.peeked.is_none() {
            self.peeked = Some(self.read_significant());
        }
        self.peeked.unwrap()
    }

    #[inline]
    pub fn next_significant(&mut self) -> TokenWithTrivia {
        self.next()
    }

    #[inline]
    pub fn peek_significant(&mut self) -> TokenWithTrivia {
        self.peek()
    }

    #[inline]
    fn read_significant(&mut self) -> TokenWithTrivia {
        let start = self.lexer.token_pos;
        let mut end = start;
        let mut first_comment_start = None;
        let mut has_white_space = false;
        loop {
            let token = self.lexer.next_token();
            if token.kind == TokenKind::WhiteSpace {
                has_white_space = true;
                end = token.range.end;
                continue;
            }
            if token.kind == TokenKind::Comment {
                first_comment_start.get_or_insert(token.range.start);
                end = token.range.end;
                continue;
            }
            let leading = Trivia {
                range: Range::new(start, end),
                end,
                first_comment_start,
                has_white_space,
            };
            return TokenWithTrivia { token, leading };
        }
    }
}

impl Lexer<'_> {
    pub fn consume_comments(&mut self) -> Option<()> {
        if self.cur()? == C_SOLIDUS && self.peek()? == C_ASTERISK {
            self.consume();
            loop {
                self.consume();
                let c = self.cur()?;
                if c == C_ASTERISK && self.peek()? == C_SOLIDUS {
                    self.consume();
                    self.consume();
                    break;
                }
            }
        }
        Some(())
    }

    pub fn consume_space(&mut self) -> Option<()> {
        self.consume();
        while is_white_space(self.cur()?) {
            self.consume();
        }
        Some(())
    }

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
}

pub fn is_new_line(c: u8) -> bool {
    c == C_LINE_FEED || c == C_CARRIAGE_RETURN || c == C_FORM_FEED
}

pub fn is_space(c: u8) -> bool {
    c == C_TAB || c == C_SPACE
}

pub fn is_white_space(c: u8) -> bool {
    is_new_line(c) || is_space(c)
}

pub fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

pub fn is_hex_digit(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

pub fn is_ident_start(c: u8) -> bool {
    c == C_LOW_LINE || c.is_ascii_alphabetic() || !c.is_ascii()
}

#[inline]
fn is_name_start_byte(c: u8) -> bool {
    c == 0 || c == C_LOW_LINE || c.is_ascii_alphabetic() || !c.is_ascii()
}

#[inline]
fn is_non_printable(c: u8) -> bool {
    matches!(c, 0x00..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F)
}

pub fn is_ident(c: u8) -> bool {
    is_ident_start(c) || is_digit(c) || c == C_HYPHEN_MINUS
}

pub fn start_ident_sequence(c1: u8, c2: u8, c3: u8) -> bool {
    if c1 == C_HYPHEN_MINUS {
        is_ident_start(c2) || c2 == C_HYPHEN_MINUS || are_valid_escape(c2, c3)
    } else {
        is_ident_start(c1) || are_valid_escape(c1, c2)
    }
}

pub fn maybe_valid_escape(c: u8) -> bool {
    c == C_REVERSE_SOLIDUS
}

pub fn are_valid_escape(c1: u8, c2: u8) -> bool {
    c1 == C_REVERSE_SOLIDUS && !is_new_line(c2)
}

pub fn start_number(c1: u8, c2: u8, c3: u8) -> bool {
    if c1 == C_PLUS_SIGN || c1 == C_HYPHEN_MINUS {
        is_digit(c2) || (c2 == C_FULL_STOP && is_digit(c3))
    } else {
        is_digit(c1) || (c1 == C_FULL_STOP && is_digit(c2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fmt::Write as _;

    fn assert_lexer_state(
        lexer: &Lexer<'_>,
        cur: Option<u8>,
        cur_pos: Option<Pos>,
        peek: Option<u8>,
        peek_pos: Option<Pos>,
        peek2: Option<u8>,
        peek2_pos: Option<Pos>,
    ) {
        assert_eq!(lexer.cur(), cur);
        assert_eq!(lexer.cur_pos(), cur_pos);
        assert_eq!(lexer.peek(), peek);
        assert_eq!(lexer.peek_pos(), peek_pos);
        assert_eq!(lexer.peek2(), peek2);
        assert_eq!(lexer.peek2_pos(), peek2_pos);
    }

    #[derive(Default)]
    struct Snapshot {
        results: Vec<(String, String)>,
    }

    impl Snapshot {
        pub fn add(&mut self, key: &str, value: &str) {
            self.results.push((key.to_string(), value.to_string()))
        }

        pub fn snapshot(&self) -> String {
            self.results
                .iter()
                .fold(String::new(), |mut output, (key, value)| {
                    writeln!(output, "{key}: {value}").expect("writing to String cannot fail");
                    output
                })
        }
    }

    impl Visitor<'_> for Snapshot {
        fn function(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("function", lexer.slice(start, end)?);
            Some(())
        }

        fn ident(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("ident", lexer.slice(start, end)?);
            Some(())
        }

        fn url(
            &mut self,
            lexer: &mut Lexer,
            _: Pos,
            _: Pos,
            content_start: Pos,
            content_end: Pos,
        ) -> Option<()> {
            self.add("url", lexer.slice(content_start, content_end)?);
            Some(())
        }

        fn string(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("string", lexer.slice(start, end)?);
            Some(())
        }

        fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
            Some(true)
        }

        fn id(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("id", lexer.slice(start, end)?);
            Some(())
        }

        fn left_parenthesis(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("left_parenthesis", lexer.slice(start, end)?);
            Some(())
        }

        fn right_parenthesis(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("right_parenthesis", lexer.slice(start, end)?);
            Some(())
        }

        fn comma(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("comma", lexer.slice(start, end)?);
            Some(())
        }

        fn class(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("class", lexer.slice(start, end)?);
            Some(())
        }

        fn pseudo_function(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("pseudo_function", lexer.slice(start, end)?);
            Some(())
        }

        fn pseudo_class(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("pseudo_class", lexer.slice(start, end)?);
            Some(())
        }

        fn semicolon(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("semicolon", lexer.slice(start, end)?);
            Some(())
        }

        fn at_keyword(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("at_keyword", lexer.slice(start, end)?);
            Some(())
        }

        fn left_curly_bracket(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("left_curly", lexer.slice(start, end)?);
            Some(())
        }

        fn right_curly_bracket(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
            self.add("right_curly", lexer.slice(start, end)?);
            Some(())
        }
    }

    fn assert_lexer_snapshot(input: &str, snapshot: &str) {
        let mut s = Snapshot::default();
        let mut l = Lexer::new(input);
        l.lex(&mut s);
        assert!(l.cur().is_none());
        similar_asserts::assert_eq!(s.snapshot(), snapshot);
    }

    #[test]
    fn streaming_visitor_adapter_preserves_basic_token_callbacks() {
        let input = r#"body { color: red; .child:hover { background: url(image.png); } }"#;
        let mut old = Snapshot::default();
        let mut old_lexer = Lexer::new(input);
        old_lexer.lex(&mut old);

        let mut streamed = Snapshot::default();
        let mut streamed_lexer = Lexer::new(input);
        streamed_lexer.lex_tokens(&mut streamed);

        similar_asserts::assert_eq!(streamed.snapshot(), old.snapshot());
    }

    #[test]
    fn streaming_visitor_adapter_preserves_unrestricted_hash_callbacks() {
        for input in ["#123 { color: red; }", "# { color: red; }"] {
            let mut old = Snapshot::default();
            let mut old_lexer = Lexer::new(input);
            old_lexer.lex(&mut old);

            let mut streamed = Snapshot::default();
            let mut streamed_lexer = Lexer::new(input);
            streamed_lexer.lex_tokens(&mut streamed);

            similar_asserts::assert_eq!(streamed.snapshot(), old.snapshot());
        }
    }

    fn token_text(input: &str, token: Token) -> &str {
        Lexer::slice_range(input, &token.range).unwrap()
    }

    #[test]
    fn streaming_tokenizer_consumes_one_to_six_hex_escape_digits() {
        for input in [
            r"\1 ",
            r"\12 ",
            r"\123 ",
            r"\1234 ",
            r"\12345 ",
            r"\123456 ",
        ] {
            let mut lexer = Lexer::new(input);
            let token = lexer.next_token();
            assert_eq!(token.kind, TokenKind::Ident);
            assert_eq!(token_text(input, token), input);
            assert_eq!(lexer.next_token().kind, TokenKind::Eof);
        }
    }

    #[test]
    fn streaming_tokenizer_keeps_consecutive_comments_distinct() {
        let input = "/** first **//** second **/ \tfoo";
        let mut lexer = Lexer::new(input);
        let first = lexer.next_token();
        let second = lexer.next_token();
        let whitespace = lexer.next_token();
        let ident = lexer.next_token();
        assert_eq!(first.kind, TokenKind::Comment);
        assert_eq!(second.kind, TokenKind::Comment);
        assert_eq!(token_text(input, first), "/** first **/");
        assert_eq!(token_text(input, second), "/** second **/");
        assert_eq!(whitespace.kind, TokenKind::WhiteSpace);
        assert_eq!(ident.kind, TokenKind::Ident);

        let mut cursor = TokenCursor::new(input);
        let significant = cursor.next_significant();
        assert_eq!(significant.token.kind, TokenKind::Ident);
        assert_eq!(significant.leading.first_comment_start, Some(0));
        assert!(significant.leading.has_whitespace());
        assert_eq!(significant.leading.range, Range::new(0, 29));
    }

    #[test]
    fn streaming_tokenizer_reports_eof_tokens_after_unterminated_inputs() {
        let mut ident = Lexer::new("ident");
        assert_eq!(ident.next_token().kind, TokenKind::Ident);
        assert_eq!(ident.next_token().kind, TokenKind::Eof);

        let mut string = Lexer::new("'unterminated");
        assert_eq!(string.next_token().kind, TokenKind::BadString);
        assert_eq!(string.next_token().kind, TokenKind::Eof);

        let mut comment = Lexer::new("/* unterminated");
        assert_eq!(comment.next_token().kind, TokenKind::BadComment);
        assert_eq!(comment.next_token().kind, TokenKind::Eof);
    }

    #[test]
    fn streaming_tokenizer_does_not_treat_string_comments_as_trivia() {
        let input = r#""/* not a comment */""#;
        let mut lexer = Lexer::new(input);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::QuotedString);
        assert_eq!(token_text(input, token), input);
        assert_eq!(lexer.next_token().kind, TokenKind::Eof);
    }

    #[test]
    fn streaming_tokenizer_scans_escaped_url_content_as_one_token() {
        let input = r"url(https:\2f\2fexample.com\2fimage.png)";
        let mut lexer = Lexer::new(input);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::Url);
        assert_eq!(token_text(input, token), input);
        assert_eq!(
            Lexer::slice_range(input, &token.value_range).unwrap(),
            r"https:\2f\2fexample.com\2fimage.png"
        );
        assert_eq!(lexer.next_token().kind, TokenKind::Eof);
    }

    #[test]
    fn streaming_tokenizer_handles_escaped_url_names_and_string_continuations() {
        let input = r"u\72l(foo)";
        let mut lexer = Lexer::new(input);
        assert_eq!(lexer.next_token().kind, TokenKind::Url);
        assert_eq!(lexer.next_token().kind, TokenKind::Eof);

        let input: String = ['"', 'a', '\\', '\n', 'b', '"'].into_iter().collect();
        let mut lexer = Lexer::new(&input);
        let token = lexer.next_token();
        assert_eq!(token.kind, TokenKind::QuotedString);
        assert_eq!(token_text(&input, token), input);
    }

    #[test]
    fn token_cursor_preserves_nested_value_at_rule_punctuation() {
        let input = "@value (foo:bar, func(a,b; c)) from \"theme.css\";";
        let mut cursor = TokenCursor::new(input);
        let mut kinds = Vec::new();
        loop {
            let token = cursor.next_significant().token;
            kinds.push(token.kind);
            if token.kind == TokenKind::Eof {
                break;
            }
        }
        assert_eq!(
            kinds,
            vec![
                TokenKind::AtKeyword,
                TokenKind::LeftParenthesis,
                TokenKind::Ident,
                TokenKind::Colon,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Function,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Ident,
                TokenKind::Semicolon,
                TokenKind::Ident,
                TokenKind::RightParenthesis,
                TokenKind::RightParenthesis,
                TokenKind::Ident,
                TokenKind::QuotedString,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexer_state_1() {
        let mut l = Lexer::new("");
        assert_lexer_state(&l, None, None, None, Some(0), None, None);
        l.consume();
        assert_eq!(l.cur(), None);
        assert_lexer_state(&l, None, Some(0), None, None, None, None);
        l.consume();
        assert_eq!(l.cur(), None);
    }

    #[test]
    fn lexer_state_2() {
        let input = "0壹👂삼";
        let bytes = input.as_bytes();
        let mut l = Lexer::new(input);
        assert_lexer_state(
            &l,
            None,
            None,
            bytes.first().copied(),
            Some(0),
            bytes.get(1).copied(),
            Some(1),
        );
        for (pos, byte) in bytes.iter().copied().enumerate() {
            l.consume();
            assert_lexer_state(
                &l,
                Some(byte),
                Some(pos as Pos),
                bytes.get(pos + 1).copied(),
                Some((pos + 1) as Pos),
                bytes.get(pos + 2).copied(),
                bytes.get(pos + 1).map(|_| (pos + 2) as Pos),
            );
        }
        l.consume();
        assert_eq!(l.cur(), None);
        assert_lexer_state(&l, None, Some(11), None, None, None, None);
        l.consume();
        assert_eq!(l.cur(), None);
    }

    #[test]
    fn slice_range_rejects_invalid_utf8_boundaries() {
        let input = "aéz";
        assert_eq!(Lexer::slice_range(input, &Range::new(1, 3)), Some("é"));
        assert!(Lexer::slice_range(input, &Range::new(1, 2)).is_none());
        assert!(Lexer::slice_range(input, &Range::new(2, 3)).is_none());
        assert!(Lexer::slice_range(input, &Range::new(0, 5)).is_none());
        assert!(Lexer::slice_range(input, &Range::new(3, 1)).is_none());
    }

    #[test]
    fn parse_urls() {
        assert_lexer_snapshot(
            indoc! {r#"
            body {
                background: url(
                    https://example\2f4a8f.com\
            /image.png
                )
            }
            --element\ name.class\ name#_id {
                background: url(  "https://example.com/some url \"with\" 'spaces'.png"   )  url('https://example.com/\'"quotes"\'.png');
            }
        "#},
            indoc! {r#"
            ident: body
            left_curly: {
            ident: background
            url: https://example\2f4a8f.com\
            /image.png
            right_curly: }
            ident: --element\ name
            class: .class\ name
            id: #_id
            left_curly: {
            ident: background
            function: url(
            string: "https://example.com/some url \"with\" 'spaces'.png"
            right_parenthesis: )
            function: url(
            string: 'https://example.com/\'"quotes"\'.png'
            right_parenthesis: )
            semicolon: ;
            right_curly: }
        "#},
        );
    }

    #[test]
    fn parse_pseudo_functions() {
        assert_lexer_snapshot(
            indoc! {r#"
            :local(.class#id, .class:not(*:hover)) { color: red; }
            :import(something from ":somewhere") {}
        "#},
            indoc! {r#"
            pseudo_function: :local(
            class: .class
            id: #id
            comma: ,
            class: .class
            pseudo_function: :not(
            pseudo_class: :hover
            right_parenthesis: )
            right_parenthesis: )
            left_curly: {
            ident: color
            ident: red
            semicolon: ;
            right_curly: }
            pseudo_function: :import(
            ident: something
            ident: from
            string: ":somewhere"
            right_parenthesis: )
            left_curly: {
            right_curly: }
        "#},
        );
    }

    #[test]
    fn parse_at_rules() {
        assert_lexer_snapshot(
            indoc! {r#"
            @media (max-size: 100px) {
                @import "external.css";
                body { color: red; }
            }
        "#},
            indoc! {r#"
            at_keyword: @media
            left_parenthesis: (
            ident: max-size
            right_parenthesis: )
            left_curly: {
            at_keyword: @import
            string: "external.css"
            semicolon: ;
            ident: body
            left_curly: {
            ident: color
            ident: red
            semicolon: ;
            right_curly: }
            right_curly: }
        "#},
        );
    }

    #[test]
    fn parse_escape() {
        assert_lexer_snapshot(
            indoc! {r#"
                body {
                    a\
                a: \
                url(https://example\2f4a8f.com\
                /image.png)
                    b: url(#\
                hash)
                }
            "#},
            indoc! {r#"
                ident: body
                left_curly: {
                ident: a\
                a
                url: https://example\2f4a8f.com\
                /image.png
                ident: b
                url: #\
                hash
                right_curly: }
            "#},
        );
    }

    #[test]
    fn parse_pseudo_elements() {
        assert_lexer_snapshot(
            indoc! {r#"
                a::after {
                    content: ' (' attr(href) ')';
                }
            "#},
            indoc! {r#"
                ident: a
                pseudo_class: :after
                left_curly: {
                ident: content
                string: ' ('
                function: attr(
                ident: href
                right_parenthesis: )
                string: ')'
                semicolon: ;
                right_curly: }
            "#},
        );
    }

    #[test]
    fn parse_minimized_urls() {
        assert_lexer_snapshot(
            "body{background:url(./image.png)}",
            indoc! {r#"
                ident: body
                left_curly: {
                ident: background
                pseudo_function: :url(
                class: .
                ident: image
                class: .png
                right_parenthesis: )
                right_curly: }
            "#},
        );
    }
}
