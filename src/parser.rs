use std::borrow::Cow;
use std::sync::Arc;

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
    fn function(&mut self, parser: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()>;
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
    tokens: Arc<[TokenAndSpan]>,
    index: usize,
}

impl<'s> CssParser<'s> {
    pub fn new(input: &'s str) -> Self {
        let mut tokenizer = Tokenizer::new(input);
        let mut tokens = Vec::new();
        while let Ok(token) = tokenizer.next() {
            tokens.push(token);
        }
        Self {
            input,
            tokens: tokens.into(),
            index: 0,
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
                    let _ = visitor.function(self, start, end);
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
        if token.token == Token::Function {
            let _ = visitor.pseudo_function(self, start, token.span.end);
        } else {
            let _ = visitor.pseudo_class(self, start, token.span.end);
        }
    }

    pub(crate) fn current(&self) -> Option<TokenAndSpan> {
        self.tokens.get(self.index).copied()
    }

    pub(crate) fn current_is(&self, kind: Token) -> bool {
        self.current().is_some_and(|token| token.token == kind)
    }

    pub(crate) fn advance(&mut self) -> Option<TokenAndSpan> {
        let token = self.current()?;
        self.index += 1;
        Some(token)
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

    pub(crate) fn token_before(&self, position: Pos) -> Option<TokenAndSpan> {
        self.tokens
            .iter()
            .rev()
            .find(|token| token.span.end <= position)
            .copied()
    }

    pub(crate) fn rebased_tokens(&self, start: Pos, end: Pos) -> SmallVec<[TokenAndSpan; 16]> {
        self.tokens
            .iter()
            .filter(|token| token.span.start >= start && token.span.end <= end)
            .map(|token| {
                let mut token = *token;
                token.span.start -= start;
                token.span.end -= start;
                token.value_span.start -= start;
                token.value_span.end -= start;
                token
            })
            .collect()
    }

    pub(crate) fn has_non_trivia_between(&self, start: Pos, end: Pos) -> bool {
        self.tokens.iter().any(|token| {
            token.span.start >= start
                && token.span.end <= end
                && !matches!(token.token, Token::WhiteSpace | Token::Comment)
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

struct TokenStream<'s> {
    input: &'s str,
    tokens: SmallVec<[TokenAndSpan; 16]>,
}

impl<'s> TokenStream<'s> {
    #[cfg(test)]
    fn new(input: &'s str) -> Self {
        let mut tokenizer = Tokenizer::new(input);
        let mut tokens = SmallVec::new();
        while let Ok(token) = tokenizer.next() {
            tokens.push(token);
        }
        Self { input, tokens }
    }

    fn raw(&self, token: &TokenAndSpan) -> &'s str {
        &self.input[token.span.start as usize..token.span.end as usize]
    }

    fn slice(&self, tokens: &[TokenAndSpan]) -> &'s str {
        let Some((first, last)) = trimmed_token_bounds(tokens) else {
            return &self.input[0..0];
        };
        &self.input[first.span.start as usize..last.span.end as usize]
    }

    fn is_keyword(&self, token: &TokenAndSpan, keyword: &str) -> bool {
        token.token == Token::Ident && escape::eq_ignore_ascii_case(self.raw(token), keyword)
    }
}

fn is_trivia(token: Token) -> bool {
    matches!(token, Token::WhiteSpace | Token::Comment)
}

fn trimmed_token_bounds(tokens: &[TokenAndSpan]) -> Option<(&TokenAndSpan, &TokenAndSpan)> {
    let first = tokens.iter().find(|token| !is_trivia(token.token))?;
    let last = tokens.iter().rfind(|token| !is_trivia(token.token))?;
    Some((first, last))
}

#[derive(Clone, Copy)]
struct TokenGroup {
    start: usize,
    end: usize,
}

#[derive(Default)]
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

fn top_level_groups(tokens: &[TokenAndSpan]) -> SmallVec<[TokenGroup; 4]> {
    let mut result = SmallVec::new();
    let mut depth = BlockDepth::default();
    let mut start = None;

    for (index, token) in tokens.iter().enumerate() {
        if depth.is_top_level() && is_trivia(token.token) {
            if let Some(start) = start.take() {
                result.push(TokenGroup { start, end: index });
            }
        } else if start.is_none() {
            start = Some(index);
        }
        depth.consume(token.token);
    }

    if let Some(start) = start {
        result.push(TokenGroup {
            start,
            end: tokens.len(),
        });
    }
    result
}

fn top_level_token(tokens: &[TokenAndSpan], kind: Token) -> Option<usize> {
    let mut depth = BlockDepth::default();
    for (index, token) in tokens.iter().enumerate() {
        if depth.is_top_level() && token.token == kind {
            return Some(index);
        }
        depth.consume(token.token);
    }
    None
}

fn strip_outer_parentheses(tokens: &[TokenAndSpan]) -> &[TokenAndSpan] {
    let Some((first, last)) = trimmed_token_bounds(tokens) else {
        return tokens;
    };
    if first.token != Token::ParenthesisBlock || last.token != Token::CloseParenthesis {
        return tokens;
    }

    let first_index = tokens
        .iter()
        .position(|token| std::ptr::eq(token, first))
        .expect("first token belongs to the slice");
    let last_index = tokens
        .iter()
        .position(|token| std::ptr::eq(token, last))
        .expect("last token belongs to the slice");
    let mut depth = 0u32;
    for (index, token) in tokens[first_index..=last_index].iter().enumerate() {
        match token.token {
            Token::Function | Token::ParenthesisBlock => depth += 1,
            Token::CloseParenthesis => {
                depth = depth.saturating_sub(1);
                if depth == 0 && first_index + index != last_index {
                    return tokens;
                }
            }
            _ => {}
        }
    }
    if depth == 0 {
        &tokens[first_index + 1..last_index]
    } else {
        tokens
    }
}

fn split_import_items(tokens: &[TokenAndSpan]) -> SmallVec<[&[TokenAndSpan]; 2]> {
    let tokens = strip_outer_parentheses(tokens);
    let mut result = SmallVec::new();
    let mut depth = BlockDepth::default();
    let mut start = 0usize;

    for (index, token) in tokens.iter().enumerate() {
        if depth.is_top_level() && token.token == Token::Comma {
            if trimmed_token_bounds(&tokens[start..index]).is_some() {
                result.push(&tokens[start..index]);
            }
            start = index + 1;
        }
        depth.consume(token.token);
    }
    if trimmed_token_bounds(&tokens[start..]).is_some() {
        result.push(&tokens[start..]);
    }
    result
}

fn parse_import_item<'s>(
    stream: &TokenStream<'s>,
    tokens: &[TokenAndSpan],
) -> ValueAtRuleImportItem<'s> {
    if let Some(colon) = top_level_token(tokens, Token::Colon) {
        return ValueAtRuleImportItem {
            local_name: stream.slice(&tokens[..colon]),
            import_name: stream.slice(&tokens[colon + 1..]),
        };
    }

    let mut depth = BlockDepth::default();
    let mut alias = None;
    for (index, token) in tokens.iter().enumerate() {
        if depth.is_top_level() && stream.is_keyword(token, "as") {
            alias = Some(index);
        }
        depth.consume(token.token);
    }
    if let Some(alias) = alias {
        let import_name = stream.slice(&tokens[..alias]);
        let local_name = stream.slice(&tokens[alias + 1..]);
        if !import_name.is_empty() && !local_name.is_empty() {
            return ValueAtRuleImportItem {
                local_name,
                import_name,
            };
        }
    }

    let name = stream.slice(tokens);
    ValueAtRuleImportItem {
        local_name: name,
        import_name: name,
    }
}

fn normalize_value<'s>(stream: &TokenStream<'s>, start: usize) -> &'s str {
    if start == stream.input.len() {
        return &stream.input[start..];
    }
    let tokens = stream
        .tokens
        .iter()
        .filter(|token| token.span.end as usize > start)
        .collect::<SmallVec<[&TokenAndSpan; 16]>>();
    if tokens.iter().all(|token| is_trivia(token.token)) {
        return &stream.input[start..];
    }

    let first = tokens
        .iter()
        .find(|token| token.token != Token::WhiteSpace)
        .expect("a non-whitespace token exists");
    let last = tokens
        .iter()
        .rfind(|token| token.token != Token::WhiteSpace)
        .expect("a non-whitespace token exists");
    &stream.input[(first.span.start as usize).max(start)..last.span.end as usize]
}

#[cfg(test)]
pub(crate) fn parse_value_at_rule(input: &str) -> ValueAtRule<'_> {
    let stream = TokenStream::new(input);
    parse_value_at_rule_stream(&stream)
}

pub(crate) fn parse_value_at_rule_tokens(
    input: &str,
    tokens: SmallVec<[TokenAndSpan; 16]>,
) -> ValueAtRule<'_> {
    parse_value_at_rule_stream(&TokenStream { input, tokens })
}

fn parse_value_at_rule_stream<'s>(stream: &TokenStream<'s>) -> ValueAtRule<'s> {
    let tokens = stream.tokens.as_slice();
    let groups = top_level_groups(tokens);

    if groups.len() >= 2 {
        let from_group = groups[groups.len() - 2];
        let request_group = groups[groups.len() - 1];
        let from_tokens = &tokens[from_group.start..from_group.end];
        if from_tokens.len() == 1 && stream.is_keyword(&from_tokens[0], "from") {
            let before_from = &tokens[..from_group.start];
            let items = split_import_items(before_from)
                .into_iter()
                .map(|tokens| parse_import_item(stream, tokens))
                .collect();
            return ValueAtRule::Import {
                from: stream.slice(&tokens[request_group.start..request_group.end]),
                items,
            };
        }
    }

    if let Some(colon) = top_level_token(tokens, Token::Colon) {
        return ValueAtRule::Value {
            local_name: stream.slice(&tokens[..colon]),
            value: normalize_value(stream, tokens[colon].span.end as usize),
            has_colon: true,
        };
    }

    let Some(name_group) = groups.first().copied() else {
        return ValueAtRule::Value {
            local_name: &stream.input[..0],
            value: &stream.input[..0],
            has_colon: false,
        };
    };
    let name_tokens = &tokens[name_group.start..name_group.end];
    let value_start = name_tokens
        .last()
        .expect("a token group is non-empty")
        .span
        .end as usize;
    ValueAtRule::Value {
        local_name: stream.slice(name_tokens),
        value: normalize_value(stream, value_start),
        has_colon: false,
    }
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
