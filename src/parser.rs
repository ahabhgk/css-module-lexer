use std::borrow::Cow;

use smallvec::SmallVec;

use crate::escape;
use crate::tokenizer::{Token, TokenAndSpan, Tokenizer};
use crate::{Pos, Range};

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

pub(crate) fn parse_value_at_rule(input: &str) -> ValueAtRule<'_> {
    let stream = TokenStream::new(input);
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
                .map(|tokens| parse_import_item(&stream, tokens))
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
            value: normalize_value(&stream, tokens[colon].span.end as usize),
            has_colon: true,
        };
    }

    let Some(name_group) = groups.first().copied() else {
        return ValueAtRule::Value {
            local_name: &input[..0],
            value: &input[..0],
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
        value: normalize_value(&stream, value_start),
        has_colon: false,
    }
}

pub(crate) fn remove_comments(input: &str) -> Cow<'_, str> {
    let stream = TokenStream::new(input);
    let comments = stream
        .tokens
        .iter()
        .filter(|token| token.token == Token::Comment);
    let mut result = None::<String>;
    let mut copied_until = 0usize;

    for comment in comments {
        let output = result.get_or_insert_with(|| String::with_capacity(input.len()));
        output.push_str(&input[copied_until..comment.span.start as usize]);
        copied_until = comment.span.end as usize;
    }
    if let Some(mut result) = result {
        result.push_str(&input[copied_until..]);
        Cow::Owned(result)
    } else {
        Cow::Borrowed(input)
    }
}

pub(crate) fn grid_template_area_ranges(input: &str, offset: Pos) -> SmallVec<[Range; 4]> {
    let stream = TokenStream::new(input);
    let mut result = SmallVec::new();
    let mut start = 0usize;

    for index in 0..=stream.tokens.len() {
        let is_boundary =
            index == stream.tokens.len() || stream.tokens[index].token == Token::WhiteSpace;
        if !is_boundary {
            continue;
        }
        if let Some((first, last)) = trimmed_token_bounds(&stream.tokens[start..index]) {
            let is_dots = stream.tokens[start..index]
                .iter()
                .all(|token| token.token == Token::Delim && stream.raw(token) == ".");
            if !is_dots {
                result.push(Range::new(
                    offset + first.span.start as Pos,
                    offset + last.span.end as Pos,
                ));
            }
        }
        start = index + 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_inside_strings_are_not_comment_tokens() {
        assert_eq!(
            remove_comments(r#""/* value */"/* comment */tail"#),
            r#""/* value */"tail"#
        );
    }

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
