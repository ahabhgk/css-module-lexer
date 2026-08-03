/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(
    dead_code,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::result_unit_err,
    clippy::should_implement_trait,
    clippy::upper_case_acronyms
)]

// https://drafts.csswg.org/css-syntax/#tokenization

use self::Token::*;
use crate::escape;
use std::ops::Range;

/// A byte range occupied by a token in the original CSS source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

/// A capture of the tokenizer state that can be restored with [`Tokenizer::reset`].
#[derive(Debug, Clone)]
pub struct TokenizerState {
    position: usize,
    current_line_start_position: usize,
    current_line_number: u32,
}

impl TokenizerState {
    /// The position from the start of the input, counted in UTF-8 bytes.
    #[inline]
    pub fn position(&self) -> SourcePosition {
        SourcePosition(self.position)
    }

    /// The line and column number at this state.
    #[inline]
    pub fn source_location(&self) -> SourceLocation {
        SourceLocation {
            line: self.current_line_number,
            column: (self.position - self.current_line_start_position + 1) as u32,
        }
    }
}

/// The lexical kind of a CSS token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Token {
    /// A [`<ident-token>`](https://drafts.csswg.org/css-syntax/#ident-token-diagram)
    Ident,

    /// A [`<at-keyword-token>`](https://drafts.csswg.org/css-syntax/#at-keyword-token-diagram)
    AtKeyword,

    /// A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to "unrestricted"
    Hash,

    /// A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to "id"
    IDHash, // Hash that is a valid ID selector.

    /// A [`<string-token>`](https://drafts.csswg.org/css-syntax/#string-token-diagram)
    QuotedString,

    /// A [`<url-token>`](https://drafts.csswg.org/css-syntax/#url-token-diagram)
    ///
    /// `url( <string-token> )` is represented by a `Function` token.
    UnquotedUrl,

    /// A `<delim-token>`
    Delim,

    /// A [`<number-token>`](https://drafts.csswg.org/css-syntax/#number-token-diagram)
    Number,

    /// A [`<percentage-token>`](https://drafts.csswg.org/css-syntax/#percentage-token-diagram)
    Percentage,

    /// A [`<dimension-token>`](https://drafts.csswg.org/css-syntax/#dimension-token-diagram)
    Dimension,

    /// A [`<whitespace-token>`](https://drafts.csswg.org/css-syntax/#whitespace-token-diagram)
    WhiteSpace,

    /// A comment.
    ///
    /// The CSS Syntax spec does not generate tokens for comments, but this tokenizer does.
    Comment,

    /// A `:` `<colon-token>`
    Colon, // :

    /// A `;` `<semicolon-token>`
    Semicolon, // ;

    /// A `,` `<comma-token>`
    Comma, // ,

    /// A `~=` [`<include-match-token>`](https://drafts.csswg.org/css-syntax/#include-match-token-diagram)
    IncludeMatch,

    /// A `|=` [`<dash-match-token>`](https://drafts.csswg.org/css-syntax/#dash-match-token-diagram)
    DashMatch,

    /// A `^=` [`<prefix-match-token>`](https://drafts.csswg.org/css-syntax/#prefix-match-token-diagram)
    PrefixMatch,

    /// A `$=` [`<suffix-match-token>`](https://drafts.csswg.org/css-syntax/#suffix-match-token-diagram)
    SuffixMatch,

    /// A `*=` [`<substring-match-token>`](https://drafts.csswg.org/css-syntax/#substring-match-token-diagram)
    SubstringMatch,

    /// A `<!--` [`<CDO-token>`](https://drafts.csswg.org/css-syntax/#CDO-token-diagram)
    CDO,

    /// A `-->` [`<CDC-token>`](https://drafts.csswg.org/css-syntax/#CDC-token-diagram)
    CDC,

    /// A [`<function-token>`](https://drafts.csswg.org/css-syntax/#function-token-diagram)
    Function,

    /// A `<(-token>`
    ParenthesisBlock,

    /// A `<[-token>`
    SquareBracketBlock,

    /// A `<{-token>`
    CurlyBracketBlock,

    /// A `<bad-url-token>`
    ///
    /// This token always indicates a parse error.
    BadUrl,

    /// A `<bad-string-token>`
    ///
    /// This token always indicates a parse error.
    BadString,

    /// A `<)-token>`
    ///
    /// When obtained from one of the `Parser::next*` methods,
    /// this token is always unmatched and indicates a parse error.
    CloseParenthesis,

    /// A `<]-token>`
    ///
    /// When obtained from one of the `Parser::next*` methods,
    /// this token is always unmatched and indicates a parse error.
    CloseSquareBracket,

    /// A `<}-token>`
    ///
    /// When obtained from one of the `Parser::next*` methods,
    /// this token is always unmatched and indicates a parse error.
    CloseCurlyBracket,
}

impl Token {
    /// Return whether this token represents a parse error.
    ///
    /// `BadUrl` and `BadString` are tokenizer-level parse errors.
    ///
    /// `CloseParenthesis`, `CloseSquareBracket`, and `CloseCurlyBracket` are *unmatched*
    /// and therefore parse errors when returned by one of the `Parser::next*` methods.
    pub fn is_parse_error(&self) -> bool {
        matches!(
            *self,
            BadUrl | BadString | CloseParenthesis | CloseSquareBracket | CloseCurlyBracket
        )
    }
}

/// A lexical token and its complete range in the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenAndSpan {
    /// The token kind.
    pub token: Token,
    /// The complete source range occupied by the token.
    pub span: Span,
    /// The raw source range containing the token value.
    ///
    /// CSS escapes inside this range are not decoded.
    pub value_span: Span,
}

impl TokenAndSpan {
    /// Creates a token with its source span.
    #[inline]
    pub const fn new(token: Token, span: Span, value_span: Span) -> Self {
        Self {
            token,
            span,
            value_span,
        }
    }
}

#[derive(Clone)]
pub struct Tokenizer<'a> {
    input: &'a str,
    /// Counted in bytes, not code points. From 0.
    position: usize,
    /// The position at the start of the current line; but adjusted to
    /// ensure that computing the column will give the result in units
    /// of UTF-16 characters.
    current_line_start_position: usize,
    current_line_number: u32,
}

impl<'a> Tokenizer<'a> {
    /// Creates a tokenizer over `input`.
    #[inline]
    pub fn new(input: &'a str) -> Self {
        assert!(
            u32::try_from(input.len()).is_ok(),
            "CSS input exceeds the maximum supported length"
        );
        Tokenizer {
            input,
            position: 0,
            current_line_start_position: 0,
            current_line_number: 0,
        }
    }

    #[inline]
    pub fn next(&mut self) -> Result<TokenAndSpan, ()> {
        let start = self.position();
        let token = next_token(self)?;
        let span = Span::new(start.0 as u32, self.position as u32);
        let value_span = token_value_span(self.input, token, span);
        Ok(TokenAndSpan::new(token, span, value_span))
    }

    #[inline]
    pub fn position(&self) -> SourcePosition {
        debug_assert!(self.input.is_char_boundary(self.position));
        SourcePosition(self.position)
    }

    #[inline]
    pub fn current_source_location(&self) -> SourceLocation {
        SourceLocation {
            line: self.current_line_number,
            column: (self.position - self.current_line_start_position + 1) as u32,
        }
    }

    #[inline]
    pub fn state(&self) -> TokenizerState {
        TokenizerState {
            position: self.position,
            current_line_start_position: self.current_line_start_position,
            current_line_number: self.current_line_number,
        }
    }

    #[inline]
    pub fn reset(&mut self, state: &TokenizerState) {
        self.position = state.position;
        self.current_line_start_position = state.current_line_start_position;
        self.current_line_number = state.current_line_number;
    }

    #[inline]
    pub(crate) fn slice_from(&self, start_pos: SourcePosition) -> &'a str {
        self.slice(start_pos..self.position())
    }

    #[inline]
    pub(crate) fn slice(&self, range: Range<SourcePosition>) -> &'a str {
        debug_assert!(self.input.is_char_boundary(range.start.0));
        debug_assert!(self.input.is_char_boundary(range.end.0));
        unsafe { self.input.get_unchecked(range.start.0..range.end.0) }
    }

    pub fn current_source_line(&self) -> &'a str {
        let current = self.position();
        let start = self
            .slice(SourcePosition(0)..current)
            .rfind(['\r', '\n', '\x0C'])
            .map_or(0, |start| start + 1);
        let end = self
            .slice(current..SourcePosition(self.input.len()))
            .find(['\r', '\n', '\x0C'])
            .map_or(self.input.len(), |end| current.0 + end);
        self.slice(SourcePosition(start)..SourcePosition(end))
    }

    #[inline]
    pub fn next_byte(&self) -> Option<u8> {
        if self.is_eof() {
            None
        } else {
            Some(self.input.as_bytes()[self.position])
        }
    }

    #[inline]
    fn is_eof(&self) -> bool {
        !self.has_at_least(0)
    }

    // If true, the input has at least `n` bytes left *after* the current one.
    // That is, `tokenizer.char_at(n)` will not panic.
    #[inline]
    fn has_at_least(&self, n: usize) -> bool {
        self.position + n < self.input.len()
    }

    // Advance over N bytes in the input.  This function can advance
    // over ASCII bytes (excluding newlines), or UTF-8 sequence
    // leaders (excluding leaders for 4-byte sequences).
    #[inline]
    pub fn advance(&mut self, n: usize) {
        if cfg!(debug_assertions) {
            // Each byte must either be an ASCII byte or a sequence
            // leader, but not a 4-byte leader; also newlines are
            // rejected.
            for i in 0..n {
                let b = self.byte_at(i);
                debug_assert!(b.is_ascii() || (b & 0xF0 != 0xF0 && b & 0xC0 != 0x80));
                debug_assert!(b != b'\r' && b != b'\n' && b != b'\x0C');
            }
        }
        self.position += n
    }

    // Assumes non-EOF
    #[inline]
    fn next_byte_unchecked(&self) -> u8 {
        self.byte_at(0)
    }

    #[inline]
    fn byte_at(&self, offset: usize) -> u8 {
        self.input.as_bytes()[self.position + offset]
    }

    // Advance over a single byte; the byte must be a UTF-8 sequence
    // leader for a 4-byte sequence.
    #[inline]
    fn consume_4byte_intro(&mut self) {
        debug_assert!(self.next_byte_unchecked() & 0xF0 == 0xF0);
        // This takes two UTF-16 characters to represent, so we
        // actually have an undercount.
        self.current_line_start_position = self.current_line_start_position.wrapping_sub(1);
        self.position += 1;
    }

    // Advance over a single byte; the byte must be a UTF-8
    // continuation byte.
    #[inline]
    fn consume_continuation_byte(&mut self) {
        debug_assert!(self.next_byte_unchecked() & 0xC0 == 0x80);
        // Continuation bytes contribute to column overcount.  Note
        // that due to the special case for the 4-byte sequence intro,
        // we must use wrapping add here.
        self.current_line_start_position = self.current_line_start_position.wrapping_add(1);
        self.position += 1;
    }

    // Advance over any kind of byte, excluding newlines.
    #[inline(never)]
    fn consume_known_byte(&mut self, byte: u8) {
        debug_assert!(byte != b'\r' && byte != b'\n' && byte != b'\x0C');
        self.position += 1;
        // Continuation bytes contribute to column overcount.
        if byte & 0xF0 == 0xF0 {
            // This takes two UTF-16 characters to represent, so we
            // actually have an undercount.
            self.current_line_start_position = self.current_line_start_position.wrapping_sub(1);
        } else if byte & 0xC0 == 0x80 {
            // Note that due to the special case for the 4-byte
            // sequence intro, we must use wrapping add here.
            self.current_line_start_position = self.current_line_start_position.wrapping_add(1);
        }
    }

    #[inline]
    fn consume_escape_sequence(&mut self) {
        let end = escape::parse_escape(self.input, self.position).end;
        while self.position < end {
            let byte = self.next_byte_unchecked();
            if matches!(byte, b'\n' | b'\r' | b'\x0C') {
                self.consume_newline();
            } else {
                self.consume_known_byte(byte);
            }
        }
    }

    // Given that a newline has been seen, advance over the newline
    // and update the state.
    #[inline]
    fn consume_newline(&mut self) {
        let byte = self.next_byte_unchecked();
        debug_assert!(byte == b'\r' || byte == b'\n' || byte == b'\x0C');
        self.position += 1;
        if byte == b'\r' && self.next_byte() == Some(b'\n') {
            self.position += 1;
        }
        self.current_line_start_position = self.position;
        self.current_line_number += 1;
    }

    #[inline]
    fn has_newline_at(&self, offset: usize) -> bool {
        self.position + offset < self.input.len()
            && matches!(self.byte_at(offset), b'\n' | b'\r' | b'\x0C')
    }

    #[inline]
    fn starts_with(&self, needle: &[u8]) -> bool {
        self.input.as_bytes()[self.position..].starts_with(needle)
    }

    pub fn skip_whitespace(&mut self) {
        while !self.is_eof() {
            match_byte! { self.next_byte_unchecked(),
                b' ' | b'\t' => {
                    self.advance(1)
                },
                b'\n' | b'\x0C' | b'\r' => {
                    self.consume_newline();
                },
                b'/' => {
                    if self.starts_with(b"/*") {
                        consume_comment(self);
                    } else {
                        return
                    }
                }
                _ => return,
            }
        }
    }

    pub fn skip_cdc_and_cdo(&mut self) {
        while !self.is_eof() {
            match_byte! { self.next_byte_unchecked(),
                b' ' | b'\t' => {
                    self.advance(1)
                },
                b'\n' | b'\x0C' | b'\r' => {
                    self.consume_newline();
                },
                b'/' => {
                    if self.starts_with(b"/*") {
                        consume_comment(self);
                    } else {
                        return
                    }
                }
                b'<' => {
                    if self.starts_with(b"<!--") {
                        self.advance(4)
                    } else {
                        return
                    }
                }
                b'-' => {
                    if self.starts_with(b"-->") {
                        self.advance(3)
                    } else {
                        return
                    }
                }
                _ => {
                    return
                }
            }
        }
    }
}

/// A position from the start of the input, counted in UTF-8 bytes.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct SourcePosition(pub(crate) usize);

impl SourcePosition {
    /// Returns the current byte index in the original input.
    #[inline]
    pub fn byte_index(&self) -> usize {
        self.0
    }
}

/// The line and column number for a given position within the input.
#[derive(PartialEq, Eq, Debug, Clone, Copy, Default)]
pub struct SourceLocation {
    /// The line number, starting at 0 for the first line.
    pub line: u32,

    /// The column number within a line, starting at 1 for first the character of the line.
    /// Column numbers are counted in UTF-16 code units.
    pub column: u32,
}

fn token_value_span(input: &str, token: Token, span: Span) -> Span {
    let raw = &input[span.start as usize..span.end as usize];
    match token {
        Token::Ident => Span::new(
            span.start,
            trim_css_whitespace_end(input, span.start, span.end),
        ),
        Token::AtKeyword | Token::IDHash => Span::new(
            span.start + 1,
            trim_css_whitespace_end(input, span.start + 1, span.end),
        ),
        Token::Hash => Span::new(span.start + 1, span.end),
        Token::Function => Span::new(
            span.start,
            trim_css_whitespace_end(input, span.start, span.end - 1),
        ),
        Token::QuotedString | Token::BadString => {
            let start = span.start + 1;
            let end = if raw.len() > 1 && raw.as_bytes().last() == raw.as_bytes().first() {
                span.end - 1
            } else {
                span.end
            };
            Span::new(start, end)
        }
        Token::Comment => {
            let start = (span.start + 2).min(span.end);
            let end = if raw.ends_with("*/") {
                span.end - 2
            } else {
                span.end
            };
            Span::new(start, end)
        }
        Token::UnquotedUrl => {
            let open = raw
                .find('(')
                .expect("an unquoted URL token contains an opening parenthesis");
            let bytes = raw.as_bytes();
            let mut start = open + 1;
            let mut end = raw.len();
            while start < end && matches!(bytes[start], b' ' | b'\t' | b'\n' | b'\r' | 0x0c) {
                start += 1;
            }
            if bytes.get(end - 1) == Some(&b')') {
                end -= 1;
            }
            while end > start && matches!(bytes[end - 1], b' ' | b'\t' | b'\n' | b'\r' | 0x0c) {
                end -= 1;
            }
            Span::new(span.start + start as u32, span.start + end as u32)
        }
        _ => span,
    }
}

fn trim_css_whitespace_end(input: &str, start: u32, end: u32) -> u32 {
    let value = &input[start as usize..end as usize];
    end - (value.len()
        - value
            .trim_end_matches([' ', '\t', '\n', '\r', '\u{c}'])
            .len()) as u32
}

fn next_token(tokenizer: &mut Tokenizer<'_>) -> Result<Token, ()> {
    if tokenizer.is_eof() {
        return Err(());
    }
    let b = tokenizer.next_byte_unchecked();
    let token = match_byte! { b,
        b' ' | b'\t' => {
            consume_whitespace(tokenizer, false)
        },
        b'\n' | b'\x0C' | b'\r' => consume_whitespace(tokenizer, true),
        b'"' => consume_string(tokenizer, false),
        b'#' => {
            tokenizer.advance(1);
            if is_ident_start(tokenizer) { consume_name(tokenizer); IDHash }
            else if !tokenizer.is_eof() &&
                matches!(tokenizer.next_byte_unchecked(), b'0'..=b'9' | b'-') {
                // Any other valid case here already resulted in IDHash.
                consume_name(tokenizer); Hash
            }
            else { Delim }
        },
        b'$' => {
            if tokenizer.starts_with(b"$=") { tokenizer.advance(2); SuffixMatch }
            else { tokenizer.advance(1); Delim }
        },
        b'\'' => consume_string(tokenizer, true),
        b'(' => { tokenizer.advance(1); ParenthesisBlock },
        b')' => { tokenizer.advance(1); CloseParenthesis },
        b'*' => {
            if tokenizer.starts_with(b"*=") { tokenizer.advance(2); SubstringMatch }
            else { tokenizer.advance(1); Delim }
        },
        b'+' => {
            if (
                tokenizer.has_at_least(1)
                && tokenizer.byte_at(1).is_ascii_digit()
            ) || (
                tokenizer.has_at_least(2)
                && tokenizer.byte_at(1) == b'.'
                && tokenizer.byte_at(2).is_ascii_digit()
            ) {
                consume_numeric(tokenizer)
            } else {
                tokenizer.advance(1);
                Delim
            }
        },
        b',' => { tokenizer.advance(1); Comma },
        b'-' => {
            if (
                tokenizer.has_at_least(1)
                && tokenizer.byte_at(1).is_ascii_digit()
            ) || (
                tokenizer.has_at_least(2)
                && tokenizer.byte_at(1) == b'.'
                && tokenizer.byte_at(2).is_ascii_digit()
            ) {
                consume_numeric(tokenizer)
            } else if tokenizer.starts_with(b"-->") {
                tokenizer.advance(3);
                CDC
            } else if is_ident_start(tokenizer) {
                consume_ident_like(tokenizer)
            } else {
                tokenizer.advance(1);
                Delim
            }
        },
        b'.' => {
            if tokenizer.has_at_least(1)
                && tokenizer.byte_at(1).is_ascii_digit() {
                consume_numeric(tokenizer)
            } else {
                tokenizer.advance(1);
                Delim
            }
        }
        b'/' => {
            if tokenizer.starts_with(b"/*") {
                consume_comment(tokenizer);
                Comment
            } else {
                tokenizer.advance(1);
                Delim
            }
        }
        b'0'..=b'9' => consume_numeric(tokenizer),
        b':' => { tokenizer.advance(1); Colon },
        b';' => { tokenizer.advance(1); Semicolon },
        b'<' => {
            if tokenizer.starts_with(b"<!--") {
                tokenizer.advance(4);
                CDO
            } else {
                tokenizer.advance(1);
                Delim
            }
        },
        b'@' => {
            tokenizer.advance(1);
            if is_ident_start(tokenizer) { consume_name(tokenizer); AtKeyword }
            else { Delim }
        },
        b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'\0' => consume_ident_like(tokenizer),
        b'[' => { tokenizer.advance(1); SquareBracketBlock },
        b'\\' => {
            if !tokenizer.has_newline_at(1) { consume_ident_like(tokenizer) }
            else { tokenizer.advance(1); Delim }
        },
        b']' => { tokenizer.advance(1); CloseSquareBracket },
        b'^' => {
            if tokenizer.starts_with(b"^=") { tokenizer.advance(2); PrefixMatch }
            else { tokenizer.advance(1); Delim }
        },
        b'{' => { tokenizer.advance(1); CurlyBracketBlock },
        b'|' => {
            if tokenizer.starts_with(b"|=") { tokenizer.advance(2); DashMatch }
            else { tokenizer.advance(1); Delim }
        },
        b'}' => { tokenizer.advance(1); CloseCurlyBracket },
        b'~' => {
            if tokenizer.starts_with(b"~=") { tokenizer.advance(2); IncludeMatch }
            else { tokenizer.advance(1); Delim }
        },
        _ => {
            if !b.is_ascii() {
                consume_ident_like(tokenizer)
            } else {
                tokenizer.advance(1);
                Delim
            }
        },
    };
    Ok(token)
}

fn consume_whitespace(tokenizer: &mut Tokenizer<'_>, newline: bool) -> Token {
    if newline {
        tokenizer.consume_newline();
    } else {
        tokenizer.advance(1);
    }
    while !tokenizer.is_eof() {
        let b = tokenizer.next_byte_unchecked();
        match_byte! { b,
            b' ' | b'\t' => {
                tokenizer.advance(1);
            }
            b'\n' | b'\x0C' | b'\r' => {
                tokenizer.consume_newline();
            }
            _ => {
                break
            }
        }
    }
    WhiteSpace
}

fn consume_comment(tokenizer: &mut Tokenizer<'_>) {
    tokenizer.advance(2); // consume "/*"
    while !tokenizer.is_eof() {
        match_byte! { tokenizer.next_byte_unchecked(),
            b'*' => {
                tokenizer.advance(1);
                if tokenizer.next_byte() == Some(b'/') {
                    tokenizer.advance(1);
                    return
                }
            }
            b'\n' | b'\x0C' | b'\r' => {
                tokenizer.consume_newline();
            }
            b'\x80'..=b'\xBF' => { tokenizer.consume_continuation_byte(); }
            b'\xF0'..=b'\xFF' => { tokenizer.consume_4byte_intro(); }
            _ => {
                // ASCII or other leading byte.
                tokenizer.advance(1);
            }
        }
    }
}

fn consume_string(tokenizer: &mut Tokenizer<'_>, single_quote: bool) -> Token {
    match consume_quoted_string(tokenizer, single_quote) {
        Ok(()) => QuotedString,
        Err(()) => BadString,
    }
}

/// Return `Err(())` on syntax error (ie. unescaped newline)
fn consume_quoted_string(tokenizer: &mut Tokenizer<'_>, single_quote: bool) -> Result<(), ()> {
    tokenizer.advance(1); // Skip the initial quote
    while !tokenizer.is_eof() {
        match_byte! { tokenizer.next_byte_unchecked(),
            b'\n' | b'\r' | b'\x0C' => {
                return Err(());
            }
            b'"' if !single_quote => {
                tokenizer.advance(1);
                return Ok(());
            }
            b'\'' if single_quote => {
                tokenizer.advance(1);
                return Ok(());
            }
            b'\\' => {
                tokenizer.consume_escape_sequence();
            }
            b'\x80'..=b'\xBF' => {
                tokenizer.consume_continuation_byte();
            }
            b'\xF0'..=b'\xFF' => {
                tokenizer.consume_4byte_intro();
            }
            _ => {
                // ASCII or other leading byte.
                tokenizer.advance(1);
            }
        }
    }

    Ok(())
}

#[inline]
fn is_ident_start(tokenizer: &Tokenizer<'_>) -> bool {
    !tokenizer.is_eof()
        && match_byte! { tokenizer.next_byte_unchecked(),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'\0' => true,
            b'-' => {
                tokenizer.has_at_least(1) && match_byte! { tokenizer.byte_at(1),
                    b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'\0' => {
                        true
                    }
                    b'\\' => !tokenizer.has_newline_at(1),
                    b => !b.is_ascii(),
                }
            },
            b'\\' => !tokenizer.has_newline_at(1),
            b => !b.is_ascii(),
        }
}

fn consume_ident_like(tokenizer: &mut Tokenizer<'_>) -> Token {
    let start = consume_name(tokenizer);
    if !tokenizer.is_eof() && tokenizer.next_byte_unchecked() == b'(' {
        let name = tokenizer.slice_from(start);
        tokenizer.advance(1);
        if escape::eq_ignore_ascii_case(name, "url") {
            consume_unquoted_url(tokenizer).unwrap_or(Function)
        } else {
            Function
        }
    } else {
        Ident
    }
}

fn consume_name(tokenizer: &mut Tokenizer<'_>) -> SourcePosition {
    // start_pos is the end of the previous token, therefore at a code point boundary
    let start_pos = tokenizer.position();
    while !tokenizer.is_eof() {
        match_byte! { tokenizer.next_byte_unchecked(),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' => tokenizer.advance(1),
            b'\\' => {
                if tokenizer.has_newline_at(1) {
                    break
                }
                tokenizer.consume_escape_sequence();
            }
            b'\0' => {
                tokenizer.advance(1);
            }
            b'\x80'..=b'\xBF' => {
                tokenizer.consume_continuation_byte();
            }
            b'\xC0'..=b'\xEF' => {
                tokenizer.advance(1);
            }
            b'\xF0'..=b'\xFF' => {
                tokenizer.consume_4byte_intro();
            }
            _ => {
                break
            }
        }
    }
    start_pos
}

fn consume_numeric(tokenizer: &mut Tokenizer<'_>) -> Token {
    // Parse [+-]?\d*(\.\d+)?([eE][+-]?\d+)?
    // But this is always called so that there is at least one digit in \d*(\.\d+)?

    if matches!(tokenizer.next_byte_unchecked(), b'+' | b'-') {
        tokenizer.advance(1);
    }

    while !tokenizer.is_eof() && tokenizer.next_byte_unchecked().is_ascii_digit() {
        tokenizer.advance(1);
    }

    if tokenizer.has_at_least(1)
        && tokenizer.next_byte_unchecked() == b'.'
        && tokenizer.byte_at(1).is_ascii_digit()
    {
        tokenizer.advance(1); // Consume '.'
        while !tokenizer.is_eof() && tokenizer.next_byte_unchecked().is_ascii_digit() {
            tokenizer.advance(1);
        }
    }

    if tokenizer.has_at_least(1)
        && matches!(tokenizer.next_byte_unchecked(), b'e' | b'E')
        && (tokenizer.byte_at(1).is_ascii_digit()
            || (tokenizer.has_at_least(2)
                && matches!(tokenizer.byte_at(1), b'+' | b'-')
                && tokenizer.byte_at(2).is_ascii_digit()))
    {
        tokenizer.advance(1);
        if matches!(tokenizer.next_byte_unchecked(), b'+' | b'-') {
            tokenizer.advance(1);
        }
        while !tokenizer.is_eof() && tokenizer.next_byte_unchecked().is_ascii_digit() {
            tokenizer.advance(1);
        }
    }

    if !tokenizer.is_eof() && tokenizer.next_byte_unchecked() == b'%' {
        tokenizer.advance(1);
        return Percentage;
    }
    if is_ident_start(tokenizer) {
        consume_name(tokenizer);
        Dimension
    } else {
        Number
    }
}

fn consume_unquoted_url(tokenizer: &mut Tokenizer<'_>) -> Result<Token, ()> {
    // This is only called after "url(", so the current position is a code point boundary.
    let start_position = tokenizer.position;
    let from_start = &tokenizer.input[tokenizer.position..];
    let mut newlines = 0;
    let mut last_newline = 0;
    let mut found_printable_char = false;
    let mut iter = from_start.bytes().enumerate();
    loop {
        let (offset, b) = match iter.next() {
            Some(item) => item,
            None => {
                tokenizer.position = tokenizer.input.len();
                break;
            }
        };
        match_byte! { b,
            b' ' | b'\t' => {},
            b'\n' | b'\x0C' => {
                newlines += 1;
                last_newline = offset;
            }
            b'\r' => {
                if from_start.as_bytes().get(offset + 1) != Some(&b'\n') {
                    newlines += 1;
                    last_newline = offset;
                }
            }
            b'"' | b'\'' => return Err(()),  // Do not advance
            b')' => {
                // Don't use advance, because we may be skipping
                // newlines here, and we want to avoid the assert.
                tokenizer.position += offset + 1;
                break
            }
            _ => {
                // Don't use advance, because we may be skipping
                // newlines here, and we want to avoid the assert.
                tokenizer.position += offset;
                found_printable_char = true;
                break
            }
        }
    }

    if newlines > 0 {
        tokenizer.current_line_number += newlines;
        // No need for wrapping_add here, because there's no possible
        // way to wrap.
        tokenizer.current_line_start_position = start_position + last_newline + 1;
    }

    if found_printable_char {
        // This function only consumed ASCII (whitespace) bytes,
        // so the current position is a code point boundary.
        return Ok(consume_unquoted_url_internal(tokenizer));
    } else {
        return Ok(UnquotedUrl);
    }

    fn consume_unquoted_url_internal(tokenizer: &mut Tokenizer<'_>) -> Token {
        while !tokenizer.is_eof() {
            match_byte! { tokenizer.next_byte_unchecked(),
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0C' => {
                    return consume_url_end(tokenizer)
                }
                b')' => {
                    tokenizer.advance(1);
                    return UnquotedUrl
                }
                b'\x01'..=b'\x08' | b'\x0B' | b'\x0E'..=b'\x1F' | b'\x7F'  // non-printable
                    | b'"' | b'\'' | b'(' => {
                    tokenizer.advance(1);
                    return consume_bad_url(tokenizer)
                },
                b'\\' => {
                    if tokenizer.has_newline_at(1) {
                        tokenizer.advance(1);
                        return consume_bad_url(tokenizer)
                    }
                    tokenizer.consume_escape_sequence();
                },
                b'\0' => {
                    tokenizer.advance(1);
                }
                b'\x80'..=b'\xBF' => {
                    tokenizer.consume_continuation_byte();
                }
                b'\xF0'..=b'\xFF' => {
                    tokenizer.consume_4byte_intro();
                }
                _ => {
                    // ASCII or other leading byte.
                    tokenizer.advance(1);
                }
            }
        }
        UnquotedUrl
    }

    fn consume_url_end(tokenizer: &mut Tokenizer<'_>) -> Token {
        while !tokenizer.is_eof() {
            match_byte! { tokenizer.next_byte_unchecked(),
                b')' => {
                    tokenizer.advance(1);
                    break
                }
                b' ' | b'\t' => { tokenizer.advance(1); }
                b'\n' | b'\x0C' | b'\r' => {
                    tokenizer.consume_newline();
                }
                b => {
                    tokenizer.consume_known_byte(b);
                    return consume_bad_url(tokenizer);
                }
            }
        }
        UnquotedUrl
    }

    fn consume_bad_url(tokenizer: &mut Tokenizer<'_>) -> Token {
        // Consume up to the closing )
        while !tokenizer.is_eof() {
            match_byte! { tokenizer.next_byte_unchecked(),
                b')' => {
                    tokenizer.advance(1);
                    return BadUrl
                }
                b'\\' => {
                    tokenizer.advance(1);
                    if matches!(tokenizer.next_byte(), Some(b')') | Some(b'\\')) {
                        tokenizer.advance(1); // Skip an escaped ')' or '\'
                    }
                }
                b'\n' | b'\x0C' | b'\r' => {
                    tokenizer.consume_newline();
                }
                b => {
                    tokenizer.consume_known_byte(b);
                }
            }
        }
        BadUrl
    }
}
