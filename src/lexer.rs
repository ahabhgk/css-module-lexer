use crate::Range;
use std::collections::VecDeque;
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
    pub flags: TokenFlags,
}

impl Token {
    #[inline]
    pub const fn new(kind: TokenKind, range: Range, value_range: Range) -> Self {
        Self::with_flags(kind, range, value_range, TokenFlags::ascii())
    }

    #[inline]
    pub const fn with_flags(
        kind: TokenKind,
        range: Range,
        value_range: Range,
        flags: TokenFlags,
    ) -> Self {
        Self {
            kind,
            range,
            value_range,
            flags,
        }
    }
}

/// Properties collected while scanning a token. They let dependency parsing
/// skip escape/null checks for the overwhelmingly common plain-ASCII path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenFlags(u8);

impl TokenFlags {
    pub const HAS_ESCAPE: Self = Self(1 << 0);
    pub const HAS_NULL: Self = Self(1 << 1);
    pub const IS_ASCII: Self = Self(1 << 2);

    #[inline]
    pub const fn ascii() -> Self {
        Self::IS_ASCII
    }

    #[inline]
    pub const fn has_escape(self) -> bool {
        self.0 & Self::HAS_ESCAPE.0 != 0
    }

    #[inline]
    pub const fn has_null(self) -> bool {
        self.0 & Self::HAS_NULL.0 != 0
    }

    #[inline]
    pub const fn is_ascii(self) -> bool {
        self.0 & Self::IS_ASCII.0 != 0
    }

    #[inline]
    fn mark_escape(&mut self) {
        self.0 |= Self::HAS_ESCAPE.0;
    }

    #[inline]
    fn mark_null(&mut self) {
        self.0 |= Self::HAS_NULL.0;
    }

    #[inline]
    fn mark_non_ascii(&mut self) {
        self.0 &= !Self::IS_ASCII.0;
    }

    #[inline]
    fn merge(&mut self, other: Self) {
        self.0 |= other.0;
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

#[derive(Debug)]
pub struct Lexer<'s> {
    value: &'s [u8],
    scan_pos: Pos,
}

impl<'s> Lexer<'s> {
    pub fn new(value: &'s str) -> Self {
        assert!(value.len() <= Pos::MAX as usize, "CSS input is too large");
        Self {
            value: value.as_bytes(),
            scan_pos: 0,
        }
    }

    #[inline]
    pub(crate) fn source_end(&self) -> Pos {
        self.value.len() as Pos
    }

    #[inline(always)]
    pub(crate) fn byte_at(&self, position: Pos) -> Option<u8> {
        self.value.get(position as usize).copied()
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
                let (kind, end, _, _, _) = self.scan_comment(position);
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

    /// Skip value text that cannot produce a dependency or change the block
    /// structure.  The next function-like name, property candidate, string,
    /// comment, or punctuation is left for the normal token path.
    pub(crate) fn fast_forward_generic_value<F>(&mut self, mut is_candidate: F)
    where
        F: FnMut(&str) -> bool,
    {
        let mut position = self.scan_pos as usize;
        let scan_start = position;
        while position < self.value.len() {
            if is_white_space(self.value[position]) {
                position += 1;
                continue;
            }
            if self.starts_ident_at(position) {
                let end = self.scan_name(position);
                if end == position {
                    break;
                }
                let name = unsafe { str::from_utf8_unchecked(&self.value[position..end]) };
                if self.value.get(end) == Some(&C_LEFT_PARENTHESIS) || is_candidate(name) {
                    break;
                }
                position = end;
                continue;
            }
            if self.starts_number_at(position) {
                position = self.scan_numeric(position).1;
                continue;
            }
            break;
        }
        self.scan_pos = position as Pos;
        debug_assert!(
            self.scan_pos >= scan_start as Pos,
            "fast_forward moved scan_pos backward from {scan_start} to {position}"
        );
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

    #[inline(always)]
    pub(crate) fn slice_trusted(&self, start: Pos, end: Pos) -> &'s str {
        debug_assert!(start <= end && end <= self.value.len() as Pos);
        // SAFETY: Token ranges and parser positions originate from this
        // lexer, so they delimit valid UTF-8 source boundaries.
        unsafe { Self::slice_unchecked(self.value, &Range::new(start, end)) }
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
    pub(crate) fn scan_pos(&self) -> Pos {
        self.scan_pos
    }
}

impl<'s> Lexer<'s> {
    /// Return the next CSS token without allocating or decoding its value.
    ///
    /// `TokenKind::Eof` is returned after the input is exhausted. Unterminated
    /// comments, strings, and URLs are represented by the corresponding
    /// `Bad*` token kinds, so EOF and tokenizer errors are never conflated.
    #[inline]
    pub fn next_token(&mut self) -> Token {
        let start = self.scan_pos as usize;
        let len = self.value.len();
        if start >= len {
            self.scan_pos = len as Pos;
            return Token::new(
                TokenKind::Eof,
                Range::new(len as Pos, len as Pos),
                Range::new(len as Pos, len as Pos),
            );
        }

        let byte = self.value[start];
        let (kind, end, value_start, value_end, flags) = match byte {
            C_SPACE | C_TAB | C_LINE_FEED | C_CARRIAGE_RETURN | C_FORM_FEED => {
                self.scan_whitespace(start)
            }
            C_QUOTATION_MARK => self.scan_string(start, C_QUOTATION_MARK),
            C_APOSTROPHE => self.scan_string(start, C_APOSTROPHE),
            C_NUMBER_SIGN => {
                let value_start = start + 1;
                if self.starts_ident_at(value_start) {
                    let (end, flags) = self.scan_name_with_flags(value_start);
                    (TokenKind::IdHash, end, value_start, end, flags)
                } else if self
                    .value
                    .get(value_start)
                    .is_some_and(|byte| is_digit(*byte) || *byte == C_HYPHEN_MINUS)
                {
                    let (end, flags) = self.scan_name_with_flags(value_start);
                    (TokenKind::Hash, end, value_start, end, flags)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_LEFT_PARENTHESIS => (
                TokenKind::LeftParenthesis,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_RIGHT_PARENTHESIS => (
                TokenKind::RightParenthesis,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_LEFT_SQUARE => (
                TokenKind::LeftSquareBracket,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_RIGHT_SQUARE => (
                TokenKind::RightSquareBracket,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_LEFT_CURLY => (
                TokenKind::LeftCurlyBracket,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_RIGHT_CURLY => (
                TokenKind::RightCurlyBracket,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_COLON => (
                TokenKind::Colon,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_SEMICOLON => (
                TokenKind::Semicolon,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_COMMA => (
                TokenKind::Comma,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
            C_PLUS_SIGN => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_HYPHEN_MINUS => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else if self.starts_ident_at(start) {
                    self.scan_ident_like(start)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_FULL_STOP => {
                if self.starts_number_at(start) {
                    self.scan_numeric(start)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_SOLIDUS => {
                if self.value.get(start + 1) == Some(&C_ASTERISK) {
                    self.scan_comment(start)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_AT_SIGN => {
                let value_start = start + 1;
                if self.starts_ident_at(value_start) {
                    let (end, flags) = self.scan_name_with_flags(value_start);
                    (TokenKind::AtKeyword, end, value_start, end, flags)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            C_REVERSE_SOLIDUS => {
                if self.is_valid_escape_at(start) {
                    self.scan_ident_like(start)
                } else {
                    (
                        TokenKind::Delim,
                        start + 1,
                        start,
                        start + 1,
                        TokenFlags::ascii(),
                    )
                }
            }
            b'0'..=b'9' => self.scan_numeric(start),
            b'$' if self.value[start..].starts_with(b"$=") => (
                TokenKind::SuffixMatch,
                start + 2,
                start,
                start + 2,
                TokenFlags::ascii(),
            ),
            b'^' if self.value[start..].starts_with(b"^=") => (
                TokenKind::PrefixMatch,
                start + 2,
                start,
                start + 2,
                TokenFlags::ascii(),
            ),
            b'|' if self.value[start..].starts_with(b"|=") => (
                TokenKind::DashMatch,
                start + 2,
                start,
                start + 2,
                TokenFlags::ascii(),
            ),
            b'~' if self.value[start..].starts_with(b"~=") => (
                TokenKind::IncludeMatch,
                start + 2,
                start,
                start + 2,
                TokenFlags::ascii(),
            ),
            b'*' if self.value[start..].starts_with(b"*=") => (
                TokenKind::SubstringMatch,
                start + 2,
                start,
                start + 2,
                TokenFlags::ascii(),
            ),
            _ if self.starts_ident_at(start) => self.scan_ident_like(start),
            _ => (
                TokenKind::Delim,
                start + 1,
                start,
                start + 1,
                TokenFlags::ascii(),
            ),
        };

        self.scan_pos = end as Pos;
        debug_assert!(
            self.scan_pos >= start as Pos,
            "scan_pos moved backward from {start} to {}",
            self.scan_pos
        );
        Token::with_flags(
            kind,
            Range::new(start as Pos, end as Pos),
            Range::new(value_start as Pos, value_end as Pos),
            flags,
        )
    }

    #[inline]
    fn scan_whitespace(&self, start: usize) -> (TokenKind, usize, usize, usize, TokenFlags) {
        let bytes = self.value;
        let mut end = start;
        while end < bytes.len() && is_white_space(bytes[end]) {
            end += 1;
        }
        (TokenKind::WhiteSpace, end, start, end, TokenFlags::ascii())
    }

    fn scan_comment(&self, start: usize) -> (TokenKind, usize, usize, usize, TokenFlags) {
        let bytes = self.value;
        let mut end = start + 2;
        let mut flags = TokenFlags::ascii();
        while end + 1 < bytes.len() {
            if bytes[end] == C_ASTERISK && bytes[end + 1] == C_SOLIDUS {
                return (TokenKind::Comment, end + 2, start + 2, end, flags);
            }
            if bytes[end] == 0 {
                flags.mark_null();
            } else if !bytes[end].is_ascii() {
                flags.mark_non_ascii();
            }
            end += if bytes[end] < 0x80 {
                1
            } else {
                self.utf8_width_at(end)
            };
        }
        (
            TokenKind::BadComment,
            bytes.len(),
            start + 2,
            bytes.len(),
            flags,
        )
    }

    fn scan_string(&self, start: usize, quote: u8) -> (TokenKind, usize, usize, usize, TokenFlags) {
        let mut end = start + 1;
        let mut flags = TokenFlags::ascii();
        while end < self.value.len() {
            let byte = self.value[end];
            if byte == quote {
                return (TokenKind::QuotedString, end + 1, start + 1, end, flags);
            }
            if is_new_line(byte) {
                return (TokenKind::BadString, end, start + 1, end, flags);
            }
            if byte == 0 {
                flags.mark_null();
            } else if !byte.is_ascii() {
                flags.mark_non_ascii();
            }
            if byte == C_REVERSE_SOLIDUS {
                flags.mark_escape();
                if self.value.get(end + 1).is_some_and(|next| !next.is_ascii()) {
                    flags.mark_non_ascii();
                }
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
        (TokenKind::BadString, end, start + 1, end, flags)
    }

    fn scan_ident_like(&self, start: usize) -> (TokenKind, usize, usize, usize, TokenFlags) {
        let (name_end, name_flags) = self.scan_name_with_flags(start);
        if self.value.get(name_end) != Some(&C_LEFT_PARENTHESIS) {
            return (TokenKind::Ident, name_end, start, name_end, name_flags);
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
                let (kind, end, value_start, value_end, url_flags) =
                    self.scan_url(start, content_start);
                let mut flags = name_flags;
                flags.merge(url_flags);
                return (kind, end, value_start, value_end, flags);
            }
        }
        (TokenKind::Function, open_end, start, name_end, name_flags)
    }

    fn scan_url(
        &self,
        start: usize,
        content_start: usize,
    ) -> (TokenKind, usize, usize, usize, TokenFlags) {
        let mut end = content_start;
        let mut flags = TokenFlags::ascii();
        while end < self.value.len() {
            let byte = self.value[end];
            if byte == C_RIGHT_PARENTHESIS {
                return (TokenKind::Url, end + 1, content_start, end, flags);
            }
            if is_white_space(byte) {
                let content_end = end;
                let close = self.skip_white_space(end);
                if self.value.get(close) == Some(&C_RIGHT_PARENTHESIS) {
                    return (TokenKind::Url, close + 1, content_start, content_end, flags);
                }
                return self.scan_bad_url(start, close, content_start, flags);
            }
            if byte == 0 {
                flags.mark_null();
            } else if !byte.is_ascii() {
                flags.mark_non_ascii();
            }
            if byte == C_QUOTATION_MARK
                || byte == C_APOSTROPHE
                || byte == C_LEFT_PARENTHESIS
                || is_non_printable(byte)
            {
                return self.scan_bad_url(start, end, content_start, flags);
            }
            if byte == C_REVERSE_SOLIDUS {
                flags.mark_escape();
                if self.value.get(end + 1).is_some_and(|next| !next.is_ascii()) {
                    flags.mark_non_ascii();
                }
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
                    return self.scan_bad_url(start, end, content_start, flags);
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
            flags,
        )
    }

    fn scan_bad_url(
        &self,
        _start: usize,
        mut end: usize,
        content_start: usize,
        mut flags: TokenFlags,
    ) -> (TokenKind, usize, usize, usize, TokenFlags) {
        while end < self.value.len() {
            let byte = self.value[end];
            if byte == 0 {
                flags.mark_null();
            } else if !byte.is_ascii() {
                flags.mark_non_ascii();
            }
            if byte == C_RIGHT_PARENTHESIS {
                end += 1;
                break;
            }
            if byte == C_REVERSE_SOLIDUS {
                flags.mark_escape();
                if self.value.get(end + 1).is_some_and(|next| !next.is_ascii()) {
                    flags.mark_non_ascii();
                }
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
        (TokenKind::BadUrl, end, content_start, end, flags)
    }

    fn scan_numeric(&self, start: usize) -> (TokenKind, usize, usize, usize, TokenFlags) {
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
            return (
                TokenKind::Percentage,
                end + 1,
                start,
                end + 1,
                TokenFlags::ascii(),
            );
        }
        if self.starts_ident_at(end) {
            let (end, flags) = self.scan_name_with_flags(end);
            return (TokenKind::Dimension, end, start, end, flags);
        }
        (TokenKind::Number, end, start, end, TokenFlags::ascii())
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
    fn scan_name(&self, position: usize) -> usize {
        self.scan_name_with_flags(position).0
    }

    #[inline]
    fn scan_name_with_flags(&self, mut position: usize) -> (usize, TokenFlags) {
        let bytes = self.value;
        let mut flags = TokenFlags::ascii();
        while position < bytes.len() {
            let ascii_start = position;
            while position < bytes.len() {
                let byte = bytes[position];
                if byte == 0 {
                    flags.mark_null();
                    position += 1;
                } else if byte.is_ascii_alphanumeric()
                    || matches!(byte, C_LOW_LINE | C_HYPHEN_MINUS)
                {
                    position += 1;
                } else {
                    break;
                }
            }
            if position != ascii_start {
                continue;
            }

            let byte = bytes[position];
            if byte == C_REVERSE_SOLIDUS {
                if self.is_valid_escape_at(position) {
                    flags.mark_escape();
                    if bytes.get(position + 1).is_some_and(|next| !next.is_ascii()) {
                        flags.mark_non_ascii();
                    }
                    position = self.scan_escape(position);
                } else {
                    break;
                }
            } else if byte.is_ascii() {
                break;
            } else {
                flags.mark_non_ascii();
                position += self.utf8_width_at(position);
            }
        }
        (position, flags)
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
}

/// The single forward stream used by dependency extraction.
///
/// The stream owns the main scanner state and gives the dependency scanner one
/// token of lookahead without cloning the lexer for every decision. Special
/// parsers use the same stream through `next_parser_token`, so consuming a
/// subgrammar never creates a second scanner over the source.
///
/// The cursor is strictly one-directional:
///
/// ```text
/// Lexer::scan_pos         farthest tokenized position, only advances
/// TokenStream::consumed   farthest semantically consumed position, only advances
/// buffered lookahead      tokens scanned but not yet consumed
/// ```
///
/// `next` consumes from the buffer first and only calls `lexer.next_token`
/// when the buffer is empty; it never moves the scanner backwards, so every
/// source range is tokenized at most once and every token is consumed at most
/// once.
pub(crate) struct TokenStream<'a, 's> {
    lexer: &'a mut Lexer<'s>,
    consumed: Pos,
    peeked: Option<TokenWithTrivia>,
    buffered: VecDeque<TokenWithTrivia>,
}

impl<'a, 's> TokenStream<'a, 's> {
    pub(crate) fn from_lexer(lexer: &'a mut Lexer<'s>) -> Self {
        Self {
            lexer,
            consumed: 0,
            peeked: None,
            buffered: VecDeque::new(),
        }
    }

    #[inline(always)]
    pub(crate) fn next<const KEEP_COMMENTS: bool>(&mut self) -> TokenWithTrivia {
        let token = if let Some(token) = self.peeked.take() {
            token
        } else if let Some(token) = self.buffered.pop_front() {
            token
        } else {
            self.read_significant::<KEEP_COMMENTS>()
        };
        self.consumed = self.consumed.max(token.token.range.end);
        debug_assert!(
            self.consumed <= self.lexer.scan_pos(),
            "consumed_pos ({}) exceeded scan_pos ({})",
            self.consumed,
            self.lexer.scan_pos()
        );
        token
    }

    /// Consume the next parser token while folding comments into its leading
    /// trivia. This is the equivalent of the old isolated cursor's behavior,
    /// but it advances the main stream and therefore preserves one scanner
    /// ownership for all subgrammars.
    #[inline]
    pub(crate) fn next_parser_token(&mut self) -> TokenWithTrivia {
        let mut item = self.next::<true>();
        if item.token.kind != TokenKind::Comment {
            return item;
        }

        let start = item.leading.range.start;
        let first_comment_start = item
            .leading
            .first_comment_start
            .or(Some(item.token.range.start));
        let mut has_white_space = item.leading.has_white_space;
        loop {
            item = self.next::<true>();
            has_white_space |= item.leading.has_white_space;
            if item.token.kind == TokenKind::Comment {
                continue;
            }
            let end = item.leading.end;
            item.leading = Trivia {
                range: Range::new(start, end),
                end,
                first_comment_start,
                has_white_space,
            };
            return item;
        }
    }

    #[inline]
    pub(crate) fn peek<const KEEP_COMMENTS: bool>(&mut self) -> TokenWithTrivia {
        if self.peeked.is_none() {
            self.peeked = self
                .buffered
                .pop_front()
                .or_else(|| Some(self.read_significant::<KEEP_COMMENTS>()));
        }
        self.peeked.unwrap()
    }

    #[inline]
    pub(crate) fn peek_after_non_comment<const KEEP_COMMENTS: bool>(
        &mut self,
    ) -> (TokenWithTrivia, bool) {
        self.peek::<KEEP_COMMENTS>();
        let mut has_whitespace = false;
        loop {
            for item in &self.buffered {
                has_whitespace |= item.leading.has_whitespace();
                if !matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment) {
                    return (*item, has_whitespace);
                }
            }
            let item = self.read_significant::<KEEP_COMMENTS>();
            has_whitespace |= item.leading.has_whitespace();
            let is_comment = matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment);
            self.buffered.push_back(item);
            if !is_comment {
                return (item, has_whitespace);
            }
        }
    }

    /// Peek the next parser token (folding comments into its leading trivia)
    /// without consuming it or any preceding comments. The stream state is
    /// unchanged: `next` later returns the same tokens in the same order.
    #[inline]
    pub(crate) fn peek_parser_token(&mut self) -> TokenWithTrivia {
        self.peek::<true>();
        let mut items: VecDeque<TokenWithTrivia> = VecDeque::new();
        if let Some(item) = self.peeked.take() {
            items.push_back(item);
        }
        items.append(&mut self.buffered);

        let mut leading_start = None;
        let mut first_comment_start = None;
        let mut has_white_space = false;
        let mut scanned: VecDeque<TokenWithTrivia> = VecDeque::new();
        loop {
            let item = if let Some(item) = items.pop_front() {
                item
            } else {
                let item = self.read_significant::<true>();
                items.push_back(item);
                continue;
            };
            leading_start.get_or_insert(item.leading.range.start);
            has_white_space |= item.leading.has_whitespace();
            if matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment) {
                first_comment_start.get_or_insert(item.token.range.start);
                scanned.push_back(item);
                continue;
            }

            let end = item.leading.end;
            let mut result = item;
            result.leading = Trivia {
                range: Range::new(leading_start.unwrap_or(end), end),
                end,
                first_comment_start: first_comment_start.or(item.leading.first_comment_start),
                has_white_space,
            };
            self.buffered = scanned;
            self.buffered.push_back(result);
            self.buffered.append(&mut items);
            self.peeked = None;
            return result;
        }
    }

    /// Peek at the next significant token, leaving any skipped comments in
    /// the buffer for `next` to consume in source order.
    #[inline]
    pub(crate) fn peek_significant_skipping_comments<const KEEP_COMMENTS: bool>(
        &mut self,
    ) -> TokenWithTrivia {
        self.peek::<KEEP_COMMENTS>();
        if let Some(item) = self.peeked {
            if !matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment) {
                return item;
            }
        }
        for item in &self.buffered {
            if !matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment) {
                return *item;
            }
        }
        loop {
            let item = self.read_significant::<KEEP_COMMENTS>();
            self.buffered.push_back(item);
            if !matches!(item.token.kind, TokenKind::Comment | TokenKind::BadComment) {
                return item;
            }
        }
    }

    /// The position of the next yet-untokenized source byte. The buffer may
    /// still hold tokens; callers must drain `next` before fast-forwarding.
    #[inline]
    pub(crate) fn fast_forward_if_buffer_empty<F>(&mut self, mut is_candidate: F)
    where
        F: FnMut(&str) -> bool,
    {
        if self.peeked.is_some() || !self.buffered.is_empty() {
            return;
        }
        self.lexer.fast_forward_generic_value(&mut is_candidate);
    }

    /// The farthest position consumed by `next`. This is the current semantic
    /// position of the dependency scanner.
    #[inline]
    pub(crate) fn consumed_pos(&self) -> Pos {
        self.consumed
    }

    #[inline]
    pub(crate) fn lexer(&self) -> &Lexer<'s> {
        self.lexer
    }

    #[inline]
    pub(crate) fn lexer_mut(&mut self) -> &mut Lexer<'s> {
        self.lexer
    }

    #[inline]
    pub(crate) fn source_end(&self) -> Pos {
        self.lexer.source_end()
    }

    #[inline(always)]
    pub(crate) fn byte_at(&self, position: Pos) -> Option<u8> {
        self.lexer.byte_at(position)
    }

    #[inline]
    pub(crate) fn slice(&self, start: Pos, end: Pos) -> Option<&'s str> {
        self.lexer.slice(start, end)
    }

    #[inline(always)]
    pub(crate) fn slice_trusted(&self, start: Pos, end: Pos) -> &'s str {
        self.lexer.slice_trusted(start, end)
    }

    #[inline(always)]
    fn read_significant<const KEEP_COMMENTS: bool>(&mut self) -> TokenWithTrivia {
        let start = self.lexer.scan_pos();
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
            if !KEEP_COMMENTS && matches!(token.kind, TokenKind::Comment | TokenKind::BadComment) {
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

#[inline]
fn is_name_start_byte(c: u8) -> bool {
    c == 0 || c == C_LOW_LINE || c.is_ascii_alphabetic() || !c.is_ascii()
}

#[inline]
fn is_non_printable(c: u8) -> bool {
    matches!(c, 0x00..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F)
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let mut stream_lexer = Lexer::new(input);
        let mut stream = TokenStream::from_lexer(&mut stream_lexer);
        let significant = stream.next_parser_token();
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
    fn streaming_tokenizer_marks_token_flags() {
        let mut plain = Lexer::new("plain");
        let plain = plain.next_token();
        assert!(plain.flags.is_ascii());
        assert!(!plain.flags.has_escape());
        assert!(!plain.flags.has_null());

        let mut escaped = Lexer::new(r"a\2db");
        let escaped = escaped.next_token();
        assert!(escaped.flags.is_ascii());
        assert!(escaped.flags.has_escape());
        assert!(!escaped.flags.has_null());

        let mut nul = Lexer::new("a\0b");
        let nul = nul.next_token();
        assert!(nul.flags.is_ascii());
        assert!(!nul.flags.has_escape());
        assert!(nul.flags.has_null());

        let mut unicode = Lexer::new("café");
        let unicode = unicode.next_token();
        assert!(!unicode.flags.is_ascii());
        assert!(!unicode.flags.has_escape());
        assert!(!unicode.flags.has_null());
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
    fn token_stream_preserves_nested_value_at_rule_punctuation() {
        let input = "@value (foo:bar, func(a,b; c)) from \"theme.css\";";
        let mut lexer = Lexer::new(input);
        let mut stream = TokenStream::from_lexer(&mut lexer);
        let mut kinds = Vec::new();
        loop {
            let token = stream.next_parser_token().token;
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
    fn token_stream_folds_trivia_and_preserves_pure_comments() {
        let input = " \t/** first **/foo";
        let mut lexer = Lexer::new(input);
        let mut stream = TokenStream::from_lexer(&mut lexer);
        let item = stream.next::<false>();
        assert_eq!(item.token.kind, TokenKind::Ident);
        assert!(item.leading.has_whitespace());
        assert_eq!(item.leading.first_comment_start, Some(2));

        let input = "/** magic **/ foo";
        let mut lexer = Lexer::new(input);
        let mut stream = TokenStream::from_lexer(&mut lexer);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Comment);
        let item = stream.next::<true>();
        assert_eq!(item.token.kind, TokenKind::Ident);
        assert!(item.leading.has_whitespace());
    }

    #[test]
    fn token_stream_peeks_past_comments_without_rescanning() {
        let mut lexer = Lexer::new("local/**/.foo");
        let mut stream = TokenStream::from_lexer(&mut lexer);
        assert_eq!(stream.peek::<true>().token.kind, TokenKind::Ident);
        let (next, has_whitespace) = stream.peek_after_non_comment::<true>();
        assert_eq!(next.token.kind, TokenKind::Delim);
        assert!(!has_whitespace);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Ident);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Comment);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Delim);
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
    fn token_stream_cursors_are_monotonic_and_cover_the_input() {
        for input in [
            ".a { color: red; }",
            ".a { width: 10px; }",
            ".a { color: red; background: blue; }",
            ":local(.a) {}",
            ":local/**/.a {}",
            ":global/**/#id {}",
            ".a { color: var(--color); }",
            ".a { background: url(\"./a.png\"); }",
            ".a { color: rgb(1, 2, 3); }",
        ] {
            let mut lexer = Lexer::new(input);
            let mut stream = TokenStream::from_lexer(&mut lexer);
            let mut previous_scan = 0;
            let mut previous_consumed = 0;
            let mut previous_end = 0;
            loop {
                let item = stream.next_parser_token();
                let scan = stream.lexer().scan_pos();
                let consumed = stream.consumed_pos();
                assert!(
                    scan >= previous_scan,
                    "scan_pos went backward in {input:?}"
                );
                assert!(
                    consumed >= previous_consumed,
                    "consumed_pos went backward in {input:?}"
                );
                assert!(
                    consumed <= scan,
                    "consumed_pos ({consumed}) exceeded scan_pos ({scan}) in {input:?}"
                );
                assert!(
                    item.token.range.start >= previous_end,
                    "token ranges are not increasing in {input:?}"
                );
                previous_end = item.token.range.end;
                previous_scan = scan;
                previous_consumed = consumed;
                if item.token.kind == TokenKind::Eof {
                    break;
                }
            }
            assert_eq!(
                previous_consumed,
                input.len() as Pos,
                "did not consume the whole input: {input:?}"
            );
            assert_eq!(
                previous_scan,
                input.len() as Pos,
                "did not scan the whole input: {input:?}"
            );
        }
    }

    #[test]
    fn token_stream_peek_past_value_does_not_rescan() {
        // The colon lookahead must not read past `red`, and `red` must be
        // consumed from the buffer instead of being tokenized twice.
        let mut lexer = Lexer::new("color:red;");
        let mut stream = TokenStream::from_lexer(&mut lexer);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Ident);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Colon);
        let red = stream.peek_significant_skipping_comments::<true>().token;
        assert_eq!(red.kind, TokenKind::Ident);
        let (semi, _) = stream.peek_after_non_comment::<true>();
        assert_eq!(semi.token.kind, TokenKind::Semicolon);
        let scan_before = stream.lexer().scan_pos();
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Ident);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Semicolon);
        assert_eq!(
            stream.lexer().scan_pos(),
            scan_before,
            "consuming buffered tokens must not advance the scanner"
        );
        assert_eq!(stream.consumed_pos(), scan_before);
    }

    #[test]
    fn token_stream_comment_separated_mode_stays_buffered_in_source_order() {
        // `:local/**/.foo` keeps the comment and `.foo` in the buffer so the
        // dependency scanner consumes them exactly once in source order.
        let mut lexer = Lexer::new(":local/**/.foo");
        let mut stream = TokenStream::from_lexer(&mut lexer);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Colon);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Ident);
        let delim = stream.peek_significant_skipping_comments::<true>().token;
        assert_eq!(delim.kind, TokenKind::Delim);
        let scan_after_peek = stream.lexer().scan_pos();
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Comment);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Delim);
        assert_eq!(
            stream.lexer().scan_pos(),
            scan_after_peek,
            "the buffered comment and delim must be consumed without rescanning"
        );
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Ident);
        assert_eq!(stream.next::<true>().token.kind, TokenKind::Eof);
    }
}
