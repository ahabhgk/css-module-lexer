use std::borrow::Cow;

use smallvec::SmallVec;

use crate::escape;
use crate::tokenizer::{Token, TokenAndSpan, Tokenizer};
use crate::{Pos, Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Trivia {
    pub(crate) end: Pos,
    pub(crate) first_comment_start: Option<Pos>,
    pub(crate) has_white_space: bool,
}

pub trait TokenVisitor<'s> {
    fn comment(&mut self, _: &mut CssParser<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }
    fn function(
        &mut self,
        parser: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        value_start: Pos,
        value_end: Pos,
    ) -> Option<()>;
    fn ident(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn url(
        &mut self,
        parser: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        content_start: Pos,
        content_end: Pos,
    ) -> Option<()>;
    fn string(
        &mut self,
        parser: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        value_start: Pos,
        value_end: Pos,
    ) -> Option<()>;
    fn is_selector(&mut self, parser: &mut CssParser<'s>) -> Option<bool>;
    fn id(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn left_parenthesis(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn right_parenthesis(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos)
        -> Option<()>;
    fn comma(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn class(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn pseudo_function(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn pseudo_class(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn semicolon(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn at_keyword(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
    fn left_curly_bracket(
        &mut self,
        parser: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
    ) -> Option<()>;
    fn right_curly_bracket(
        &mut self,
        parser: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
    ) -> Option<()>;
}

#[derive(Clone)]
pub struct CssParser<'s> {
    input: &'s str,
    /// The tokenizer is always positioned after `current`.
    tokenizer: Tokenizer<'s>,
    current: Option<TokenAndSpan>,
    previous_non_trivia: Option<TokenAndSpan>,
    last_non_trivia_end_before_semicolon: Option<Pos>,
    leading_trivia: LeadingTrivia,
    last_consumed_leading_trivia: LeadingTrivia,
    pseudo_context: Option<PseudoContext>,
}

#[derive(Clone, Copy, Default)]
struct LeadingTrivia {
    start: Option<Pos>,
    first_comment_start: Option<Pos>,
    has_white_space: bool,
}

impl LeadingTrivia {
    fn consume(&mut self, token: TokenAndSpan) {
        if self.start.is_none() {
            self.start = Some(token.span.start);
        }
        if token.token == Token::Comment {
            self.first_comment_start.get_or_insert(token.span.start);
        } else if token.token == Token::WhiteSpace {
            self.has_white_space = true;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct PseudoContext {
    previous_non_trivia: Option<TokenAndSpan>,
    leading_trivia: LeadingTrivia,
}

impl<'s> CssParser<'s> {
    pub fn new(input: &'s str) -> Self {
        let mut tokenizer = Tokenizer::new(input);
        let current = tokenizer.next().ok();
        Self {
            input,
            tokenizer,
            current,
            previous_non_trivia: None,
            last_non_trivia_end_before_semicolon: None,
            leading_trivia: LeadingTrivia::default(),
            last_consumed_leading_trivia: LeadingTrivia::default(),
            pseudo_context: None,
        }
    }

    pub fn parse<V: TokenVisitor<'s>>(&mut self, visitor: &mut V) {
        while let Some(token) = self.current() {
            let start = token.span.start;
            let end = token.span.end;
            match token.token {
                Token::Comment => {
                    self.advance();
                    let _ = visitor.comment(self, start, end);
                }
                Token::WhiteSpace
                | Token::Number
                | Token::Percentage
                | Token::Dimension
                | Token::IncludeMatch
                | Token::DashMatch
                | Token::PrefixMatch
                | Token::SuffixMatch
                | Token::SubstringMatch
                | Token::CDO
                | Token::CDC
                | Token::BadUrl
                | Token::BadString
                | Token::SquareBracketBlock
                | Token::CloseSquareBracket => {
                    self.advance();
                }
                Token::QuotedString => {
                    self.advance();
                    let _ = visitor.string(
                        self,
                        start,
                        end,
                        token.value_span.start,
                        token.value_span.end,
                    );
                }
                Token::UnquotedUrl => {
                    self.advance();
                    let _ = visitor.url(
                        self,
                        start,
                        end,
                        token.value_span.start,
                        token.value_span.end,
                    );
                }
                Token::Ident => {
                    self.advance();
                    let _ = visitor.ident(self, start, self.event_end(token));
                }
                Token::Function => {
                    self.advance();
                    let _ = visitor.function(
                        self,
                        start,
                        end,
                        token.value_span.start,
                        token.value_span.end,
                    );
                }
                Token::AtKeyword => {
                    self.advance();
                    let _ = visitor.at_keyword(self, start, end);
                }
                Token::IDHash => {
                    self.advance();
                    if visitor.is_selector(self).unwrap_or(false) {
                        let _ = visitor.id(self, start, self.event_end(token));
                    }
                }
                Token::Hash => {
                    self.advance();
                    if visitor.is_selector(self).unwrap_or(false) {
                        let _ = visitor.id(self, start, start + 1);
                    }
                }
                Token::ParenthesisBlock => {
                    self.advance();
                    let _ = visitor.left_parenthesis(self, start, end);
                }
                Token::CloseParenthesis => {
                    self.advance();
                    let _ = visitor.right_parenthesis(self, start, end);
                }
                Token::Comma => {
                    self.advance();
                    let _ = visitor.comma(self, start, end);
                }
                Token::Semicolon => {
                    self.advance();
                    let _ = visitor.semicolon(self, start, end);
                }
                Token::CurlyBracketBlock => {
                    self.advance();
                    let _ = visitor.left_curly_bracket(self, start, end);
                }
                Token::CloseCurlyBracket => {
                    self.advance();
                    let _ = visitor.right_curly_bracket(self, start, end);
                }
                Token::Colon => self.parse_pseudo(visitor),
                Token::Delim if self.raw(token) == "." => self.parse_class(visitor),
                Token::Delim if self.raw(token) == "#" => {
                    self.advance();
                    if visitor.is_selector(self).unwrap_or(false) {
                        let _ = visitor.id(self, start, end);
                    }
                }
                Token::Delim => {
                    self.advance();
                }
            }
        }
    }

    fn parse_class<V: TokenVisitor<'s>>(&mut self, visitor: &mut V) {
        let start = self.position();
        self.advance();
        if !visitor.is_selector(self).unwrap_or(false) {
            return;
        }
        let end = if self.current_is(Token::Ident) {
            let end = self.event_end(self.current().unwrap());
            self.advance();
            end
        } else {
            start + 1
        };
        let _ = visitor.class(self, start, end);
    }

    pub(crate) fn parse_pseudo<V: TokenVisitor<'s>>(&mut self, visitor: &mut V) {
        let start = self.position();
        let pseudo_context = PseudoContext {
            previous_non_trivia: self.previous_non_trivia,
            leading_trivia: self.leading_trivia,
        };
        self.advance();
        let Some(token) = self.current() else {
            return;
        };
        if !matches!(token.token, Token::Ident | Token::Function)
            || !visitor.is_selector(self).unwrap_or(false)
        {
            return;
        }
        self.advance();
        self.pseudo_context = Some(pseudo_context);
        if token.token == Token::Function {
            let _ = visitor.pseudo_function(self, start, token.span.end);
        } else {
            let _ = visitor.pseudo_class(self, start, token.span.end);
        }
        self.pseudo_context = None;
    }

    pub(crate) fn current(&self) -> Option<TokenAndSpan> {
        self.current
    }

    pub(crate) fn current_is(&self, kind: Token) -> bool {
        self.current().is_some_and(|token| token.token == kind)
    }

    pub(crate) fn advance(&mut self) -> Option<TokenAndSpan> {
        let token = self.current()?;
        if token.token == Token::CloseParenthesis {
            self.last_consumed_leading_trivia = self.leading_trivia;
        }
        if matches!(token.token, Token::WhiteSpace | Token::Comment) {
            self.leading_trivia.consume(token);
        } else {
            if token.token == Token::Semicolon {
                self.last_non_trivia_end_before_semicolon =
                    self.previous_non_trivia.map(|token| token.span.end);
            }
            self.previous_non_trivia = Some(token);
            if self.leading_trivia.start.is_some() {
                self.leading_trivia = LeadingTrivia::default();
            }
        }
        self.current = self.tokenizer.next().ok();
        Some(token)
    }

    pub(crate) fn last_consumed_trivia_start(&self) -> Option<Pos> {
        self.last_consumed_leading_trivia.start
    }

    pub(crate) fn last_non_trivia_end_before_semicolon(&self) -> Option<Pos> {
        self.last_non_trivia_end_before_semicolon
    }

    pub(crate) fn pseudo_previous_non_trivia(&self) -> Option<TokenAndSpan> {
        self.pseudo_context
            .and_then(|context| context.previous_non_trivia)
    }

    pub(crate) fn pseudo_leading_has_white_space(&self) -> bool {
        self.pseudo_context
            .is_some_and(|context| context.leading_trivia.has_white_space)
    }

    pub(crate) fn position(&self) -> Pos {
        self.current()
            .map_or(self.input.len() as Pos, |token| token.span.start)
    }

    pub(crate) fn slice(&self, start: Pos, end: Pos) -> Option<&'s str> {
        self.input.get(start as usize..end as usize)
    }

    pub fn slice_range<'a>(input: &'a str, range: &Range) -> Option<&'a str> {
        input.get(range.start as usize..range.end as usize)
    }

    pub(crate) fn raw(&self, token: TokenAndSpan) -> &'s str {
        &self.input[token.span.start as usize..token.span.end as usize]
    }

    pub(crate) fn current_raw(&self) -> Option<&'s str> {
        self.current().map(|token| self.raw(token))
    }

    pub(crate) fn token_is_keyword(&self, token: TokenAndSpan, keyword: &str) -> bool {
        matches!(
            token.token,
            Token::Ident | Token::AtKeyword | Token::Function
        ) && escape::eq_ignore_ascii_case(self.raw_token_name(token), keyword)
    }

    pub(crate) fn current_is_keyword(&self, keyword: &str) -> bool {
        self.current()
            .is_some_and(|token| self.token_is_keyword(token, keyword))
    }

    pub(crate) fn raw_token_name(&self, token: TokenAndSpan) -> &'s str {
        &self.input[token.value_span.start as usize..token.value_span.end as usize]
    }

    pub(crate) fn decoded_token_name(&self, token: TokenAndSpan) -> Cow<'s, str> {
        escape::unescape(self.raw_token_name(token))
    }

    pub(crate) fn event_end(&self, token: TokenAndSpan) -> Pos {
        if matches!(token.token, Token::Ident | Token::IDHash) {
            token.value_span.end
        } else {
            token.span.end
        }
    }

    pub(crate) fn position_after_chars(&self, count: usize) -> Pos {
        let start = self.position() as usize;
        let offset = self.input[start..]
            .char_indices()
            .nth(count)
            .map_or(self.input.len() - start, |(offset, _)| offset);
        (start + offset) as Pos
    }

    pub(crate) fn consume_trivia(&mut self) -> Trivia {
        let mut first_comment_start = None;
        let mut has_white_space = false;
        while let Some(token) = self.current() {
            match token.token {
                Token::Comment => {
                    first_comment_start.get_or_insert(token.span.start);
                    self.advance();
                }
                Token::WhiteSpace => {
                    has_white_space = true;
                    self.advance();
                }
                _ => break,
            }
        }
        Trivia {
            end: self.position(),
            first_comment_start,
            has_white_space,
        }
    }

    pub(crate) fn consume_trivia_with_visitor<V: TokenVisitor<'s>>(
        &mut self,
        visitor: &mut V,
    ) -> Option<Trivia> {
        let start = self.position();
        let mut first_comment_start = None;
        let mut has_white_space = false;
        while let Some(token) = self.current() {
            match token.token {
                Token::Comment => {
                    first_comment_start.get_or_insert(token.span.start);
                    self.advance();
                    visitor.comment(self, token.span.start, token.span.end)?;
                }
                Token::WhiteSpace => {
                    has_white_space = true;
                    self.advance();
                }
                _ => break,
            }
        }
        Some(Trivia {
            end: self.position().max(start),
            first_comment_start,
            has_white_space,
        })
    }

    pub(crate) fn skip_trivia(&mut self) {
        self.consume_trivia();
    }
}

#[derive(Debug)]
pub(crate) enum ValueAtRule<'s> {
    Import {
        from: &'s str,
        items: SmallVec<[ValueAtRuleImportItem<'s>; 2]>,
    },
    Value {
        local_name: &'s str,
        value: &'s str,
        has_colon: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueAtRuleImportItem<'s> {
    pub(crate) local_name: &'s str,
    pub(crate) import_name: &'s str,
}

fn is_trivia(token: Token) -> bool {
    matches!(token, Token::WhiteSpace | Token::Comment)
}

#[derive(Clone, Copy, Default)]
struct BlockDepth {
    parentheses: u32,
    square_brackets: u32,
    curly_brackets: u32,
}

impl BlockDepth {
    fn is_top_level(&self) -> bool {
        self.parentheses == 0 && self.square_brackets == 0 && self.curly_brackets == 0
    }

    fn consume(&mut self, token: Token) {
        match token {
            Token::Function | Token::ParenthesisBlock => self.parentheses += 1,
            Token::CloseParenthesis => {
                self.parentheses = self.parentheses.saturating_sub(1);
            }
            Token::SquareBracketBlock => self.square_brackets += 1,
            Token::CloseSquareBracket => {
                self.square_brackets = self.square_brackets.saturating_sub(1);
            }
            Token::CurlyBracketBlock => self.curly_brackets += 1,
            Token::CloseCurlyBracket => {
                self.curly_brackets = self.curly_brackets.saturating_sub(1);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
struct ValueRange {
    start: Pos,
    end: Pos,
}

impl ValueRange {
    const fn new(start: Pos, end: Pos) -> Self {
        Self { start, end }
    }

    fn slice(self, input: &str) -> &str {
        &input[self.start as usize..self.end as usize]
    }
}

#[derive(Clone, Copy)]
struct ValueGroup {
    range: ValueRange,
    is_from: bool,
}

#[derive(Clone, Copy, Default)]
struct ValueGroupBuilder {
    start: Option<Pos>,
    end: Pos,
    count: u32,
    is_from: bool,
}

impl ValueGroupBuilder {
    fn is_empty(self) -> bool {
        self.start.is_none()
    }

    fn consume(&mut self, input: &str, token: TokenAndSpan) {
        self.start.get_or_insert(token.span.start);
        if self.count == 0 {
            self.is_from = token.token == Token::Ident
                && escape::eq_ignore_ascii_case(
                    &input[token.value_span.start as usize..token.value_span.end as usize],
                    "from",
                );
        } else {
            self.is_from = false;
        }
        self.count += 1;
        self.end = token.span.end;
    }

    fn finish(self) -> Option<ValueGroup> {
        Some(ValueGroup {
            range: ValueRange::new(self.start?, self.end),
            is_from: self.is_from && self.count == 1,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueItemDelimiterKind {
    Colon,
    As,
}

#[derive(Clone, Copy)]
struct ValueItemDelimiter {
    kind: ValueItemDelimiterKind,
    start: Pos,
    end: Pos,
    before_end: Pos,
}

#[derive(Clone, Copy, Default)]
struct ValueItemBuilder {
    start: Option<Pos>,
    full_end: Pos,
    last_end: Pos,
    delimiter: Option<ValueItemDelimiter>,
    after_start: Option<Pos>,
    after_end: Pos,
}

impl ValueItemBuilder {
    fn is_empty(self) -> bool {
        self.start.is_none()
    }

    fn include(&mut self, token: TokenAndSpan) {
        self.start.get_or_insert(token.span.start);
        self.full_end = token.span.end;
        self.last_end = token.span.end;
        if self.delimiter.is_some() {
            self.after_start.get_or_insert(token.span.start);
            self.after_end = token.span.end;
        }
    }

    fn include_delimiter(&mut self, token: TokenAndSpan, kind: ValueItemDelimiterKind) {
        self.start.get_or_insert(token.span.start);
        self.delimiter = Some(ValueItemDelimiter {
            kind,
            start: token.span.start,
            end: token.span.end,
            before_end: self.last_end,
        });
        self.full_end = token.span.end;
        self.last_end = token.span.end;
        self.after_start = None;
        self.after_end = token.span.end;
    }

    fn full_range(self) -> Option<ValueRange> {
        Some(ValueRange::new(self.start?, self.full_end))
    }

    fn before_range(self) -> Option<ValueRange> {
        let delimiter = self.delimiter?;
        let start = self.start?;
        Some(ValueRange::new(start, delimiter.before_end))
    }

    fn after_range(self) -> Option<ValueRange> {
        let delimiter = self.delimiter?;
        let start = self.after_start.unwrap_or(delimiter.end);
        Some(ValueRange::new(start, self.after_end.max(start)))
    }
}

#[derive(Clone, Copy)]
struct ValueNormalization {
    start: Pos,
    first_non_white: Option<Pos>,
    last_non_white_end: Option<Pos>,
    all_trivia: bool,
}

impl ValueNormalization {
    fn new(start: Pos) -> Self {
        Self {
            start,
            first_non_white: None,
            last_non_white_end: None,
            all_trivia: true,
        }
    }

    fn consume(&mut self, token: TokenAndSpan) {
        if token.span.end <= self.start {
            return;
        }
        if token.token != Token::WhiteSpace {
            self.first_non_white
                .get_or_insert(token.span.start.max(self.start));
            self.last_non_white_end = Some(token.span.end);
        }
        if !is_trivia(token.token) {
            self.all_trivia = false;
        }
    }

    fn finish(self, input: &str, end: Pos) -> &str {
        if self.all_trivia {
            &input[self.start as usize..end as usize]
        } else {
            &input
                [self.first_non_white.unwrap() as usize..self.last_non_white_end.unwrap() as usize]
        }
    }
}

struct ValueAtRuleParser<'s> {
    input: &'s str,
    empty: Pos,
    depth: BlockDepth,
    group: ValueGroupBuilder,
    first_group: Option<ValueGroup>,
    second_last_group: Option<ValueGroup>,
    last_group: Option<ValueGroup>,
    group_count: usize,
    bare_value: Option<ValueNormalization>,
    colon_end: Option<Pos>,
    pre_colon_start: Option<Pos>,
    pre_colon_end: Pos,
    colon_value: Option<ValueNormalization>,
    items: SmallVec<[ValueItemBuilder; 4]>,
    item: ValueItemBuilder,
    pending_item_before_group: Option<ValueItemBuilder>,
    from_item: Option<ValueItemBuilder>,
    outer_parentheses: bool,
    outer_opened: bool,
    outer_closed: bool,
}

impl<'s> ValueAtRuleParser<'s> {
    fn new(input: &'s str, empty: Pos) -> Self {
        Self {
            input,
            empty,
            depth: BlockDepth::default(),
            group: ValueGroupBuilder::default(),
            first_group: None,
            second_last_group: None,
            last_group: None,
            group_count: 0,
            bare_value: None,
            colon_end: None,
            pre_colon_start: None,
            pre_colon_end: empty,
            colon_value: None,
            items: SmallVec::new(),
            item: ValueItemBuilder::default(),
            pending_item_before_group: None,
            from_item: None,
            outer_parentheses: false,
            outer_opened: false,
            outer_closed: false,
        }
    }

    fn finish_group(&mut self) {
        let Some(group) = std::mem::take(&mut self.group).finish() else {
            return;
        };
        if self.group_count == 0 {
            self.first_group = Some(group);
            self.bare_value = Some(ValueNormalization::new(group.range.end));
        } else {
            self.second_last_group = self.last_group;
        }
        self.last_group = Some(group);
        self.group_count += 1;
        if group.is_from {
            self.from_item = self.pending_item_before_group;
        }
    }

    fn item_level(&self) -> bool {
        if self.outer_parentheses && !self.outer_closed {
            self.depth.parentheses == 1
                && self.depth.square_brackets == 0
                && self.depth.curly_brackets == 0
        } else {
            self.depth.is_top_level()
        }
    }

    fn consume_item_token(&mut self, token: TokenAndSpan, was_top_level: bool) {
        if is_trivia(token.token) {
            return;
        }

        let is_outer_open = !self.outer_opened
            && was_top_level
            && token.token == Token::ParenthesisBlock
            && self.group_count == 0;
        if is_outer_open {
            self.outer_parentheses = true;
            self.outer_opened = true;
        }

        let is_outer_close = self.outer_parentheses
            && !self.outer_closed
            && token.token == Token::CloseParenthesis
            && self.depth.parentheses == 1
            && self.depth.square_brackets == 0
            && self.depth.curly_brackets == 0;
        if is_outer_close {
            self.outer_closed = true;
        }

        if is_outer_open || is_outer_close {
            return;
        }

        let item_level = self.item_level();
        if item_level && token.token == Token::Comma {
            if !self.item.is_empty() {
                self.items.push(self.item);
            }
            self.item = ValueItemBuilder::default();
            return;
        }

        if item_level && token.token == Token::Colon && self.item.delimiter.is_none() {
            self.item
                .include_delimiter(token, ValueItemDelimiterKind::Colon);
        } else if item_level
            && token.token == Token::Ident
            && matches!(
                self.item.delimiter,
                None | Some(ValueItemDelimiter {
                    kind: ValueItemDelimiterKind::As,
                    ..
                })
            )
            && escape::eq_ignore_ascii_case(
                &self.input[token.value_span.start as usize..token.value_span.end as usize],
                "as",
            )
        {
            self.item
                .include_delimiter(token, ValueItemDelimiterKind::As);
        } else {
            self.item.include(token);
        }
    }

    fn consume(&mut self, token: TokenAndSpan) {
        let trivia = is_trivia(token.token);
        let was_top_level = self.depth.is_top_level();

        if was_top_level && trivia {
            self.finish_group();
        } else if was_top_level && !trivia && self.group.is_empty() {
            self.pending_item_before_group = (!self.item.is_empty()).then_some(self.item);
        }

        if self.colon_end.is_none() && !trivia && token.token != Token::Colon {
            self.pre_colon_start.get_or_insert(token.span.start);
            self.pre_colon_end = token.span.end;
        }

        if was_top_level && token.token == Token::Colon && self.colon_end.is_none() {
            self.colon_end = Some(token.span.end);
            self.colon_value = Some(ValueNormalization::new(token.span.end));
        } else if let Some(value) = &mut self.colon_value {
            value.consume(token);
        }

        if let Some(value) = &mut self.bare_value {
            value.consume(token);
        }

        if !trivia {
            if was_top_level && self.group.is_empty() {
                self.pending_item_before_group = (!self.item.is_empty()).then_some(self.item);
            }
            self.group.consume(self.input, token);
        }
        self.consume_item_token(token, was_top_level);
        self.depth.consume(token.token);
    }

    fn import_item(&self, item: ValueItemBuilder) -> ValueAtRuleImportItem<'s> {
        if let Some(delimiter) = item.delimiter {
            let before = item
                .before_range()
                .unwrap_or(ValueRange::new(delimiter.start, delimiter.start));
            let after = item
                .after_range()
                .unwrap_or(ValueRange::new(delimiter.end, delimiter.end));
            if delimiter.kind == ValueItemDelimiterKind::Colon {
                return ValueAtRuleImportItem {
                    local_name: before.slice(self.input),
                    import_name: after.slice(self.input),
                };
            }
            let import_name = before.slice(self.input);
            let local_name = after.slice(self.input);
            if !import_name.is_empty() && !local_name.is_empty() {
                return ValueAtRuleImportItem {
                    local_name,
                    import_name,
                };
            }
        }

        let range = item
            .full_range()
            .unwrap_or(ValueRange::new(self.empty, self.empty));
        let name = range.slice(self.input);
        ValueAtRuleImportItem {
            local_name: name,
            import_name: name,
        }
    }

    fn finish(mut self, end: Pos) -> ValueAtRule<'s> {
        self.finish_group();

        if self.group_count >= 2 {
            let from_group = self.second_last_group.unwrap();
            let request_group = self.last_group.unwrap();
            if from_group.is_from {
                let mut items = SmallVec::new();
                for item in &self.items {
                    if item
                        .full_range()
                        .is_some_and(|range| range.end <= from_group.range.start)
                    {
                        items.push(self.import_item(*item));
                    }
                }
                if let Some(item) = self.from_item {
                    if item
                        .full_range()
                        .is_some_and(|range| range.end <= from_group.range.start)
                    {
                        items.push(self.import_item(item));
                    }
                }
                return ValueAtRule::Import {
                    from: request_group.range.slice(self.input),
                    items,
                };
            }
        }

        if let Some(colon_end) = self.colon_end {
            let local_name = self
                .pre_colon_start
                .map(|start| ValueRange::new(start, self.pre_colon_end))
                .unwrap_or(ValueRange::new(colon_end, colon_end))
                .slice(self.input);
            let value = self
                .colon_value
                .unwrap_or_else(|| ValueNormalization::new(colon_end))
                .finish(self.input, end);
            return ValueAtRule::Value {
                local_name,
                value,
                has_colon: true,
            };
        }

        let Some(name_group) = self.first_group else {
            let empty = &self.input[self.empty as usize..self.empty as usize];
            return ValueAtRule::Value {
                local_name: empty,
                value: empty,
                has_colon: false,
            };
        };
        let value = self
            .bare_value
            .unwrap_or_else(|| ValueNormalization::new(name_group.range.end))
            .finish(self.input, end);
        ValueAtRule::Value {
            local_name: name_group.range.slice(self.input),
            value,
            has_colon: false,
        }
    }
}

impl<'s> CssParser<'s> {
    pub(crate) fn parse_value_at_rule(&mut self) -> Option<(ValueAtRule<'s>, Pos)> {
        let empty = self.position();
        let mut parser = ValueAtRuleParser::new(self.input, empty);
        loop {
            let token = self.current()?;
            if token.token == Token::Semicolon {
                self.advance();
                let end = self.position();
                return Some((parser.finish(token.span.start), end));
            }
            if token.token == Token::CloseCurlyBracket {
                return Some((parser.finish(token.span.start), token.span.start));
            }
            parser.consume(self.advance()?);
        }
    }
}

#[cfg(test)]
pub(crate) fn parse_value_at_rule(input: &str) -> ValueAtRule<'_> {
    let mut lexer = CssParser::new(input);
    let mut parser = ValueAtRuleParser::new(input, 0);
    while let Some(token) = lexer.advance() {
        parser.consume(token);
    }
    parser.finish(input.len() as Pos)
}

pub(crate) fn grid_template_area_ranges(input: &str, offset: Pos) -> SmallVec<[Range; 4]> {
    let mut result = SmallVec::new();
    let mut position = 0;

    while position < input.len() {
        while position < input.len() {
            let (value, end) = decoded_char(input, position);
            if value.is_some_and(is_css_whitespace) {
                position = end;
            } else {
                break;
            }
        }
        if position == input.len() {
            break;
        }

        let start = position;
        let mut is_dots = true;
        while position < input.len() {
            let (value, end) = decoded_char(input, position);
            if value.is_some_and(is_css_whitespace) {
                break;
            }
            if value.is_some_and(|value| value != '.') {
                is_dots = false;
            }
            position = end;
        }
        if !is_dots {
            result.push(Range::new(offset + start as Pos, offset + position as Pos));
        }
    }
    result
}

fn decoded_char(input: &str, position: usize) -> (Option<char>, usize) {
    if input.as_bytes()[position] == b'\\' {
        let escape = escape::parse_escape(input, position);
        (escape.value, escape.end)
    } else if input.as_bytes()[position] == b'\0' {
        (Some('\u{FFFD}'), position + 1)
    } else {
        let value = input[position..].chars().next().unwrap();
        (Some(value), position + value.len_utf8())
    }
}

fn is_css_whitespace(value: char) -> bool {
    matches!(value, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_value_keywords() {
        let ValueAtRule::Import { from, items } =
            parse_value_at_rule(r#"blue a\73  primary fr\6f m "./colors.css""#)
        else {
            panic!("expected import");
        };
        assert_eq!(from, r#""./colors.css""#);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].import_name, "blue");
        assert_eq!(items[0].local_name, "primary");
    }

    #[test]
    fn preserves_comment_text_in_value_strings() {
        let ValueAtRule::Value { value, .. } =
            parse_value_at_rule(r#"literal: "/* not a comment */""#)
        else {
            panic!("expected value");
        };
        assert_eq!(value, r#""/* not a comment */""#);
    }
}
