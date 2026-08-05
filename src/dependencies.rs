//! Dependency parser and lexer visitor implementation.

use std::borrow::Cow;

use rustc_hash::FxHashSet;
use smallvec::SmallVec;

use crate::dependency_types::{
    Dependency, DependencyContext, DependencyListRange, Mode, Range, UrlRangeKind,
    ValueAtRuleImportItem, Warning, WarningKind,
};
use crate::escape::unescape;
use crate::lexer::Token;
use crate::lexer::TokenFlags;
use crate::lexer::TokenKind;
use crate::lexer::TokenStream;
use crate::HandleWarning;
use crate::Lexer;
use crate::Pos;

#[inline]
fn unescape_token(value: &str, flags: TokenFlags) -> Cow<'_, str> {
    if flags.has_escape() || flags.has_null() {
        unescape(value)
    } else {
        Cow::Borrowed(value)
    }
}

macro_rules! match_ignore_ascii_case {
    (
        $value:expr, {
            $(
                $($pattern:literal)|+ => $result:expr,
            )*
            _ => $fallback:expr $(,)?
        }
    ) => {{
        let value = $value;
        $(
            if $(value.eq_ignore_ascii_case($pattern))||+ {
                $result
            } else
        )*
        {
            $fallback
        }
    }};
}

#[derive(Debug)]
enum Scope<'s> {
    TopLevel,
    InBlock,
    InAtImport(ImportData<'s>),
    AtImportInvalid,
    AtNamespaceInvalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanContext {
    TopLevel,
    BlockItem,
    Selector,
    DeclarationName,
    GenericValue,
    SpecialValue(PropertyKind),
    AtRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropertyKind {
    Generic,
    Animation,
    ListStyle,
    FontPalette,
    Container,
    Grid,
    Composes,
    CustomProperty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtRuleKind {
    Namespace,
    Import,
    Charset,
    Value,
    Keyframes,
    Container,
    Function,
    Property,
    CounterStyle,
    FontPaletteValues,
    Scope,
    Other,
}

impl ScanContext {
    fn for_property(property: PropertyKind) -> Self {
        if property == PropertyKind::Generic {
            Self::GenericValue
        } else {
            Self::SpecialValue(property)
        }
    }
}

#[derive(Debug)]
struct ImportData<'s> {
    start: Pos,
    prelude: ImportPrelude<'s>,
    url: Option<&'s str>,
    url_flags: TokenFlags,
    url_range: Option<Range>,
    supports: ImportDataSupports<'s>,
    layer: ImportDataLayer<'s>,
}

impl ImportData<'_> {
    pub fn new(start: Pos) -> Self {
        Self {
            start,
            prelude: ImportPrelude::default(),
            url: None,
            url_flags: TokenFlags::ascii(),
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

#[derive(Debug, Default)]
struct ImportPrelude<'s>(SmallVec<[ImportPreludeNode<'s>; 2]>);

impl<'s> ImportPrelude<'s> {
    pub fn push(&mut self, node: ImportPreludeNode<'s>) {
        self.0.push(node);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn icss_import_url(&self) -> Option<(&'s str, &Range)> {
        let [ImportPreludeNode::IcssUrlCandidate { name, range }] = self.0.as_slice() else {
            return None;
        };
        Some((name, range))
    }

    pub fn first_non_url_before(&self, url_range: &Range) -> Option<&Range> {
        self.0.iter().find_map(|node| {
            let range = node.range();
            if range.start >= url_range.start || matches!(node, ImportPreludeNode::Url { .. }) {
                None
            } else {
                Some(range)
            }
        })
    }
}

#[derive(Debug)]
enum ImportPreludeNode<'s> {
    IcssUrlCandidate { name: &'s str, range: Range },
    Url { range: Range },
    Layer { range: Range },
    Supports { range: Range },
    Other { range: Range },
}

impl ImportPreludeNode<'_> {
    fn range(&self) -> &Range {
        match self {
            Self::IcssUrlCandidate { range, .. }
            | Self::Url { range }
            | Self::Layer { range }
            | Self::Supports { range }
            | Self::Other { range } => range,
        }
    }
}

#[derive(Debug)]
enum ImportDataSupports<'s> {
    None,
    InSupports,
    EndSupports { value: &'s str, range: Range },
}

#[derive(Debug)]
enum ImportDataLayer<'s> {
    None,
    EndLayer { value: &'s str, range: Range },
}

#[derive(Debug, Default)]
struct BalancedStack(SmallVec<[BalancedItem; 3]>);

impl BalancedStack {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn last(&self) -> Option<&BalancedItem> {
        self.0.last()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, item: BalancedItem, mode_data: Option<&mut ModeData>) {
        if let Some(mode_data) = mode_data {
            if item.kind.is_mode_local() {
                mode_data.set_current_mode(Mode::Local);
            } else if item.kind.is_mode_global() {
                mode_data.set_current_mode(Mode::Global);
            }

            if item.kind.is_mode_function() {
                mode_data.inside_mode_function += 1;
            } else if item.kind.is_mode_class() {
                mode_data.inside_mode_class += 1;
            }
        }
        self.0.push(item);
    }

    pub fn pop(&mut self, mode_data: Option<&mut ModeData>) -> Option<BalancedItem> {
        let item = self.0.pop()?;
        if let Some(mode_data) = mode_data {
            if item.kind.is_mode_function() {
                mode_data.inside_mode_function -= 1;
            } else if item.kind.is_mode_class() {
                mode_data.inside_mode_class -= 1;
            }
            self.update_current_mode(mode_data);
        }
        Some(item)
    }

    pub fn pop_without_moda_data(&mut self) -> Option<BalancedItem> {
        self.0.pop()
    }

    pub fn pop_mode_pseudo_class(&mut self, mode_data: &mut ModeData) {
        loop {
            if let Some(last) = self.0.last() {
                if matches!(
                    last.kind,
                    BalancedItemKind::LocalClass | BalancedItemKind::GlobalClass
                ) {
                    mode_data.inside_mode_class -= 1;
                    self.0.pop();
                    continue;
                }
            }
            break;
        }
        self.update_current_mode(mode_data);
    }

    pub fn update_current_mode(&self, mode_data: &mut ModeData) {
        mode_data.set_current_mode(self.topmost_mode(mode_data));
    }

    pub fn update_property_mode(&self, mode_data: &mut ModeData) {
        mode_data.set_property_mode(self.topmost_mode(mode_data));
    }

    fn topmost_mode(&self, mode_data: &ModeData) -> Mode {
        let mut iter = self.0.iter();
        loop {
            if let Some(last) = iter.next_back() {
                if matches!(
                    last.kind,
                    BalancedItemKind::LocalFn | BalancedItemKind::LocalClass
                ) {
                    return Mode::Local;
                } else if matches!(
                    last.kind,
                    BalancedItemKind::GlobalFn | BalancedItemKind::GlobalClass
                ) {
                    return Mode::Global;
                }
            } else {
                return mode_data.default_mode();
            }
        }
    }
}

#[derive(Debug)]
struct BalancedItem {
    kind: BalancedItemKind,
    range: Range,
}

impl BalancedItem {
    pub fn new(name: &str, start: Pos, end: Pos) -> Self {
        Self {
            kind: BalancedItemKind::new(name),
            range: Range::new(start, end),
        }
    }

    pub fn new_other(start: Pos, end: Pos) -> Self {
        Self {
            kind: BalancedItemKind::Other,
            range: Range::new(start, end),
        }
    }

    pub fn new_curly(start: Pos, end: Pos) -> Self {
        Self {
            kind: BalancedItemKind::Curly,
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
    PaletteMix,
    LocalFn,
    GlobalFn,
    LocalClass,
    GlobalClass,
    Curly,
    Other,
}

impl BalancedItemKind {
    pub fn new(name: &str) -> Self {
        match name {
            "url(" => Self::Url,
            "image-set(" => Self::ImageSet,
            _ if with_vendor_prefixed_eq(name, "image-set(", false) => Self::ImageSet,
            _ if name.eq_ignore_ascii_case("layer(") => Self::Layer,
            _ if name.eq_ignore_ascii_case("supports(") => Self::Supports,
            "palette-mix(" => Self::PaletteMix,
            _ if name.eq_ignore_ascii_case(":local(") => Self::LocalFn,
            _ if name.eq_ignore_ascii_case(":global(") => Self::GlobalFn,
            _ if name.eq_ignore_ascii_case(":local") => Self::LocalClass,
            _ if name.eq_ignore_ascii_case(":global") => Self::GlobalClass,
            _ => Self::Other,
        }
    }

    pub fn is_mode_local(&self) -> bool {
        matches!(self, Self::LocalFn | Self::LocalClass)
    }

    pub fn is_mode_global(&self) -> bool {
        matches!(self, Self::GlobalFn | Self::GlobalClass)
    }

    pub fn is_mode_function(&self) -> bool {
        matches!(self, Self::LocalFn | Self::GlobalFn)
    }

    pub fn is_mode_class(&self) -> bool {
        matches!(self, Self::LocalClass | Self::GlobalClass)
    }
}

#[derive(Debug)]
enum ValueAtRule<'s> {
    Import {
        from: &'s str,
        items: DependencyListRange<ValueAtRuleImportItem<'s>>,
    },
    Value {
        local_name: &'s str,
        value: &'s str,
        has_colon: bool,
    },
}

fn is_css_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn is_css_white_space_char(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn unescape_names(names: SmallVec<[&str; 2]>) -> impl Iterator<Item = Cow<'_, str>> {
    names.into_iter().map(unescape)
}

fn trim_slice(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut end = bytes.len();
    while start < end && is_css_space_byte(bytes[start]) {
        start += 1;
    }
    while end > start && is_css_space_byte(bytes[end - 1]) {
        end -= 1;
    }
    &input[start..end]
}

fn is_css_modules_magic_comment(input: &str, keyword: &str) -> bool {
    let input = trim_slice(input);
    let Some(rest) = input.strip_prefix(keyword) else {
        return false;
    };
    rest.is_empty()
        || rest
            .as_bytes()
            .first()
            .is_some_and(|byte| is_css_space_byte(*byte))
}

fn is_css_modules_pure_magic_comment(input: &str) -> bool {
    let input = trim_slice(input);
    let Some(keyword) = input.strip_prefix("cssmodules-pure-") else {
        return false;
    };
    ["ignore", "no-check"].into_iter().any(|expected| {
        keyword.strip_prefix(expected).is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| is_css_space_byte(*byte))
        })
    })
}

fn trivia_only(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    let bytes = input.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if is_css_space_byte(bytes[position]) {
            position += 1;
            continue;
        }
        if position + 1 < bytes.len() && bytes[position] == b'/' && bytes[position + 1] == b'*' {
            position += 2;
            while position + 1 < bytes.len()
                && !(bytes[position] == b'*' && bytes[position + 1] == b'/')
            {
                position += 1;
            }
            if position + 1 >= bytes.len() {
                return false;
            }
            position += 2;
            continue;
        }
        return false;
    }
    true
}

fn token_text(input: &str, token: Token) -> &str {
    Lexer::slice_range(input, &token.range).unwrap_or("")
}

fn token_span<'s>(input: &'s str, tokens: &[Token], start: usize, end: usize) -> &'s str {
    if start >= end {
        return "";
    }
    let start = tokens[start].range.start as usize;
    let end = tokens[end - 1].range.end as usize;
    &input[start..end]
}

fn is_open_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Function
            | TokenKind::LeftParenthesis
            | TokenKind::LeftSquareBracket
            | TokenKind::LeftCurlyBracket
    )
}

fn is_close_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RightParenthesis | TokenKind::RightSquareBracket | TokenKind::RightCurlyBracket
    )
}

fn ident_like_range(token: Token) -> Option<Range> {
    match token.kind {
        TokenKind::Ident => Some(token.range),
        TokenKind::Function => Some(token.value_range),
        _ => None,
    }
}

fn parse_value_at_rule_import_item<'s>(
    input: &'s str,
    tokens: &[Token],
    start: usize,
    end: usize,
) -> ValueAtRuleImportItem<'s> {
    if start >= end {
        return ValueAtRuleImportItem::new("", "");
    }
    let mut depth = 0u32;
    let mut colon_index = None;
    let mut as_index = None;
    for (index, token) in tokens[start..end].iter().copied().enumerate() {
        let index = start + index;
        if is_close_token(token.kind) {
            depth = depth.saturating_sub(1);
        }
        if depth == 0 {
            if token.kind == TokenKind::Colon && colon_index.is_none() {
                colon_index = Some(index);
            }
            if token.kind == TokenKind::Ident && token_text(input, token).eq_ignore_ascii_case("as")
            {
                as_index = Some(index);
            }
        }
        if is_open_token(token.kind) {
            depth += 1;
        }
    }

    if let Some(index) = colon_index {
        return ValueAtRuleImportItem::new(
            token_span(input, tokens, start, index),
            token_span(input, tokens, index + 1, end),
        );
    }
    if let Some(index) = as_index {
        let import_name = token_span(input, tokens, start, index);
        let local_name = token_span(input, tokens, index + 1, end);
        if !import_name.is_empty() && !local_name.is_empty() {
            return ValueAtRuleImportItem::new(local_name, import_name);
        }
    }

    let value = token_span(input, tokens, start, end);
    ValueAtRuleImportItem::new(value, value)
}

fn parse_value_at_rule_value(input: &str, start: Pos, has_tokens: bool) -> &str {
    let value = &input[start as usize..];
    if has_tokens {
        // A comment is part of a non-empty ICSS value. Only ordinary CSS
        // whitespace is trimmed, matching the legacy aligned-slice behavior
        // without manufacturing an equal-sized comment mask.
        trim_slice(value)
    } else {
        value
    }
}

fn parse_value_at_rule<'s>(
    input: &'s str,
    tokens: &[Token],
    dependency_context: &mut DependencyContext<'s>,
) -> ValueAtRule<'s> {
    let mut from_index = None;
    let mut depth = 0u32;
    let mut colon_index = None;
    for (index, token) in tokens.iter().copied().enumerate() {
        if is_close_token(token.kind) {
            depth = depth.saturating_sub(1);
        }
        if depth == 0
            && token.kind == TokenKind::Ident
            && token_text(input, token).eq_ignore_ascii_case("from")
            && index > 0
            && index + 1 < tokens.len()
            && trivia_only(&input[tokens[index - 1].range.end as usize..token.range.start as usize])
            && trivia_only(&input[token.range.end as usize..tokens[index + 1].range.start as usize])
        {
            from_index = Some(index);
        }
        if depth == 0 && token.kind == TokenKind::Colon && colon_index.is_none() {
            colon_index = Some(index);
        }
        if is_open_token(token.kind) {
            depth += 1;
        }
    }

    if let Some(from_index) = from_index {
        let path_index = from_index + 1;
        let before_from = token_span(input, tokens, 0, from_index);
        let from = token_span(input, tokens, path_index, path_index + 1);
        if path_index + 1 == tokens.len() && !before_from.is_empty() && !from.is_empty() {
            let mut item_start = 0;
            let mut item_end = from_index;
            if item_end >= 2
                && tokens[item_start].kind == TokenKind::LeftParenthesis
                && tokens[item_end - 1].kind == TokenKind::RightParenthesis
            {
                item_start += 1;
                item_end -= 1;
            }

            let items_start = dependency_context.value_at_rule_import_items_len();
            let mut start = item_start;
            let mut item_depth = 0u32;
            for index in item_start..item_end {
                let token = tokens[index];
                if is_close_token(token.kind) {
                    item_depth = item_depth.saturating_sub(1);
                }
                if token.kind == TokenKind::Comma && item_depth == 0 {
                    let item = parse_value_at_rule_import_item(input, tokens, start, index);
                    if !item.local_name().is_empty() || !item.import_name().is_empty() {
                        dependency_context.push_value_at_rule_import_item(item);
                    }
                    start = index + 1;
                }
                if is_open_token(token.kind) {
                    item_depth += 1;
                }
            }
            let item = parse_value_at_rule_import_item(input, tokens, start, item_end);
            if !item.local_name().is_empty() || !item.import_name().is_empty() {
                dependency_context.push_value_at_rule_import_item(item);
            }
            let items = DependencyListRange::<ValueAtRuleImportItem<'s>>::from_bounds(
                items_start,
                dependency_context.value_at_rule_import_items_len(),
            );
            return ValueAtRule::Import { from, items };
        }
    }

    if let Some(index) = colon_index {
        return ValueAtRule::Value {
            local_name: token_span(input, tokens, 0, index),
            value: parse_value_at_rule_value(
                input,
                tokens[index].range.end,
                index + 1 < tokens.len(),
            ),
            has_colon: true,
        };
    }

    let local_name = token_span(input, tokens, 0, usize::from(!tokens.is_empty()));
    let value = if tokens.len() > 1 {
        parse_value_at_rule_value(input, tokens[0].range.end, true)
    } else {
        ""
    };
    ValueAtRule::Value {
        local_name,
        value,
        has_colon: false,
    }
}

fn with_vendor_prefixed_eq(left: &str, right: &str, at_rule: bool) -> bool {
    let left = if at_rule {
        if let Some(left) = left.strip_prefix('@') {
            left
        } else {
            return false;
        }
    } else {
        left
    };
    matches!(left.strip_prefix("-webkit-"), Some(left) if left.eq_ignore_ascii_case(right))
        || matches!(left.strip_prefix("-moz-"), Some(left) if left.eq_ignore_ascii_case(right))
        || matches!(left.strip_prefix("-ms-"), Some(left) if left.eq_ignore_ascii_case(right))
        || matches!(left.strip_prefix("-o-"), Some(left) if left.eq_ignore_ascii_case(right))
}

#[derive(Debug)]
pub struct ModeData<'s> {
    default: Mode,
    current: Mode,
    property: Mode,
    resulting_global: Option<Pos>,
    pure_global: Option<Pos>,
    pure_no_check: bool,
    pure_ignore_pending: bool,
    pure_ignored_block_nesting_level: Option<u32>,
    composes_local_classes: ComposesLocalClasses<'s>,
    inside_mode_function: u32,
    inside_mode_class: u32,
}

impl ModeData<'_> {
    pub fn new(default: Mode) -> Self {
        Self {
            default,
            current: default,
            property: default,
            resulting_global: None,
            pure_global: Some(0),
            pure_no_check: false,
            pure_ignore_pending: false,
            pure_ignored_block_nesting_level: None,
            composes_local_classes: ComposesLocalClasses::default(),
            inside_mode_function: 0,
            inside_mode_class: 0,
        }
    }

    pub fn is_pure_mode(&self) -> bool {
        matches!(self.default, Mode::Pure)
    }

    pub fn mark_pure_ignore(&mut self) {
        if self.is_pure_mode() {
            self.pure_ignore_pending = true;
        }
    }

    pub fn mark_pure_no_check(&mut self) {
        if self.is_pure_mode() {
            self.pure_no_check = true;
        }
    }

    pub fn is_pure_check_disabled(&self) -> bool {
        self.pure_no_check
            || self.pure_ignore_pending
            || self.pure_ignored_block_nesting_level.is_some()
    }

    pub fn enter_block(&mut self, block_nesting_level: u32) {
        if self.pure_ignore_pending {
            self.pure_ignore_pending = false;
            if self.pure_ignored_block_nesting_level.is_none() {
                self.pure_ignored_block_nesting_level = Some(block_nesting_level);
            }
        }
    }

    pub fn clear_pure_ignore_pending(&mut self) {
        self.pure_ignore_pending = false;
    }

    pub fn exit_block(&mut self, block_nesting_level: u32) {
        if self
            .pure_ignored_block_nesting_level
            .is_some_and(|level| block_nesting_level < level)
        {
            self.pure_ignored_block_nesting_level = None;
        }
    }

    pub fn is_current_local_mode(&self) -> bool {
        match self.current {
            Mode::Local | Mode::Pure => true,
            Mode::Global | Mode::Css => false,
        }
    }

    pub fn is_property_local_mode(&self) -> bool {
        match self.property {
            Mode::Local | Mode::Pure => true,
            Mode::Global | Mode::Css => false,
        }
    }

    pub fn default_mode(&self) -> Mode {
        self.default
    }

    pub fn set_current_mode(&mut self, mode: Mode) {
        self.current = mode;
    }

    pub fn set_property_mode(&mut self, mode: Mode) {
        self.property = mode;
    }

    pub fn is_inside_mode_function(&self) -> bool {
        self.inside_mode_function > 0
    }

    pub fn is_inside_mode_class(&self) -> bool {
        self.inside_mode_class > 0
    }

    pub fn is_mode_explicit(&self) -> bool {
        self.is_inside_mode_function() || self.is_inside_mode_class()
    }
}

#[derive(Debug, Default, Clone)]
struct ComposesLocalClasses<'s> {
    is_single: SingleLocalClass,
    local_classes: SmallVec<[&'s str; 2]>,
}

impl<'s> ComposesLocalClasses<'s> {
    pub fn get_valid_local_classes(&mut self, lexer: &Lexer<'s>) -> Option<SmallVec<[&'s str; 2]>> {
        if let SingleLocalClass::Single(range) = &self.is_single {
            let mut local_classes = self.local_classes.clone();
            local_classes.push(lexer.slice(range.start, range.end)?);
            Some(local_classes)
        } else {
            self.reset_to_initial();
            None
        }
    }

    pub fn invalidate(&mut self) {
        if !matches!(self.is_single, SingleLocalClass::AtKeyword) {
            self.is_single = SingleLocalClass::Invalid;
            self.local_classes.clear();
        }
    }

    pub fn find_local_class(&mut self, start: Pos, end: Pos) {
        match self.is_single {
            SingleLocalClass::Initial => {
                self.is_single = SingleLocalClass::Single(Range::new(start, end))
            }
            SingleLocalClass::Single(_) => {
                self.is_single = SingleLocalClass::Invalid;
                self.local_classes.clear();
            }
            _ => {}
        };
    }

    pub fn find_at_keyword(&mut self) {
        self.is_single = SingleLocalClass::AtKeyword;
        self.local_classes.clear();
    }

    pub fn reset_to_initial(&mut self) {
        self.is_single = SingleLocalClass::Initial;
        self.local_classes.clear();
    }

    pub fn find_comma(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        if let SingleLocalClass::Single(range) = &self.is_single {
            self.local_classes
                .push(lexer.slice(range.start, range.end)?);
            self.is_single = SingleLocalClass::Initial
        } else {
            self.is_single = SingleLocalClass::Invalid;
        }
        Some(())
    }
}

#[derive(Debug, Default, Clone)]
enum SingleLocalClass {
    #[default]
    Initial,
    Single(Range),
    AtKeyword,
    Invalid,
}

#[derive(Debug)]
struct InProperty<T: ReservedValues> {
    reserved: T,
    rename: Option<(Range, TokenFlags)>,
    balanced_len: usize,
}

impl<T: ReservedValues> InProperty<T> {
    pub fn new(reserved: T, balanced_len: usize) -> Self {
        Self {
            reserved,
            rename: None,
            balanced_len,
        }
    }

    fn check_reserved(&mut self, ident: &str) -> bool {
        self.reserved.check(ident)
    }

    pub fn reset_reserved(&mut self) {
        self.reserved.reset();
    }

    pub fn set_rename(&mut self, ident: &str, range: Range, flags: TokenFlags) {
        if self.check_reserved(ident) {
            self.rename = Some((range, flags));
        }
    }

    pub fn take_rename(&mut self, balanced_len: usize) -> Option<(Range, TokenFlags)> {
        // Don't rename when we in functions
        if balanced_len != self.balanced_len {
            return None;
        }
        std::mem::take(&mut self.rename)
    }
}

trait ReservedValues {
    fn check(&mut self, ident: &str) -> bool;
    fn reset(&mut self);
}

#[derive(Debug, Default)]
struct AnimationReserved {
    bits: u32,
}

impl ReservedValues for AnimationReserved {
    fn check(&mut self, ident: &str) -> bool {
        match_ignore_ascii_case!(ident, {
            "normal" => self.check_and_update(Self::NORMAL),
            "reverse" => self.check_and_update(Self::REVERSE),
            "alternate" => self.check_and_update(Self::ALTERNATE),
            "alternate-reverse" => self.check_and_update(Self::ALTERNATE_REVERSE),
            "forwards" => self.check_and_update(Self::FORWARDS),
            "backwards" => self.check_and_update(Self::BACKWARDS),
            "both" => self.check_and_update(Self::BOTH),
            "infinite" => self.check_and_update(Self::INFINITE),
            "paused" => self.check_and_update(Self::PAUSED),
            "running" => self.check_and_update(Self::RUNNING),
            "ease" => self.check_and_update(Self::EASE),
            "ease-in" => self.check_and_update(Self::EASE_IN),
            "ease-out" => self.check_and_update(Self::EASE_OUT),
            "ease-in-out" => self.check_and_update(Self::EASE_IN_OUT),
            "linear" => self.check_and_update(Self::LINEAR),
            "step-end" => self.check_and_update(Self::STEP_END),
            "step-start" => self.check_and_update(Self::STEP_START),
            // keywords values
            "none" |
            // global values
            "initial" | "inherit" | "unset" | "revert" | "revert-layer" => false,
            _ => true,
        })
    }

    fn reset(&mut self) {
        self.bits = 0;
    }
}

impl AnimationReserved {
    const NORMAL: u32 = 1 << 0;
    const REVERSE: u32 = 1 << 1;
    const ALTERNATE: u32 = 1 << 2;
    const ALTERNATE_REVERSE: u32 = 1 << 3;
    const FORWARDS: u32 = 1 << 4;
    const BACKWARDS: u32 = 1 << 5;
    const BOTH: u32 = 1 << 6;
    const INFINITE: u32 = 1 << 7;
    const PAUSED: u32 = 1 << 8;
    const RUNNING: u32 = 1 << 9;
    const EASE: u32 = 1 << 10;
    const EASE_IN: u32 = 1 << 11;
    const EASE_OUT: u32 = 1 << 12;
    const EASE_IN_OUT: u32 = 1 << 13;
    const LINEAR: u32 = 1 << 14;
    const STEP_END: u32 = 1 << 15;
    const STEP_START: u32 = 1 << 16;

    fn check_and_update(&mut self, bit: u32) -> bool {
        if self.bits & bit == bit {
            return true;
        }
        self.bits |= bit;
        false
    }
}

#[derive(Debug, Default)]
struct ListStyleReserved;

impl ReservedValues for ListStyleReserved {
    fn check(&mut self, ident: &str) -> bool {
        match_ignore_ascii_case!(ident, {
            // https://www.w3.org/TR/css-counter-styles-3/#simple-numeric
            "decimal"
            | "decimal-leading-zero"
            | "arabic-indic"
            | "armenian"
            | "upper-armenian"
            | "lower-armenian"
            | "bengali"
            | "cambodian"
            | "khmer"
            | "cjk-decimal"
            | "devanagari"
            | "georgian"
            | "gujarati"
            | "gurmukhi"
            | "hebrew"
            | "kannada"
            | "lao"
            | "malayalam"
            | "mongolian"
            | "myanmar"
            | "oriya"
            | "persian"
            | "lower-roman"
            | "upper-roman"
            | "tamil"
            | "telugu"
            | "thai"
            | "tibetan"
            // https://www.w3.org/TR/css-counter-styles-3/#simple-alphabetic
            | "lower-alpha"
            | "lower-latin"
            | "upper-alpha"
            | "upper-latin"
            | "lower-greek"
            | "hiragana"
            | "hiragana-iroha"
            | "katakana"
            | "katakana-iroha"
            // https://www.w3.org/TR/css-counter-styles-3/#simple-symbolic
            | "disc"
            | "circle"
            | "square"
            | "disclosure-open"
            | "disclosure-closed"
            // https://www.w3.org/TR/css-counter-styles-3/#simple-fixed
            | "cjk-earthly-branch"
            | "cjk-heavenly-stem"
            // https://www.w3.org/TR/css-counter-styles-3/#complex-cjk
            | "japanese-informal"
            | "japanese-formal"
            | "korean-hangul-formal"
            | "korean-hanja-informal"
            | "korean-hanja-formal"
            | "simp-chinese-informal"
            | "simp-chinese-formal"
            | "trad-chinese-informal"
            | "trad-chinese-formal"
            | "ethiopic-numeric"
            // keywords values
            | "none"
            // global values
            | "initial"
            | "inherit"
            | "unset"
            | "revert"
            | "revert-layer" => false,
            _ => true,
        })
    }

    fn reset(&mut self) {}
}

#[derive(Debug, Default)]
struct FontPaletteReserved;

impl ReservedValues for FontPaletteReserved {
    fn check(&mut self, ident: &str) -> bool {
        ident.starts_with("--")
    }

    fn reset(&mut self) {}
}

#[derive(Debug, Default)]
struct ContainerReserved;

impl ReservedValues for ContainerReserved {
    fn check(&mut self, ident: &str) -> bool {
        match_ignore_ascii_case!(ident, {
            "normal"
                | "size"
                | "inline-size"
                | "scroll-state"
                | "none"
                | "initial"
                | "inherit"
                | "unset"
                | "revert"
                | "revert-layer" => false,
            _ => true,
        })
    }

    fn reset(&mut self) {}
}

#[derive(Debug, Clone, Copy)]
enum GridPropertyKind {
    Generic,
    TemplateLike,
    TemplateAreas,
}

fn grid_property_kind(ident: &str) -> Option<GridPropertyKind> {
    match_ignore_ascii_case!(ident, {
        "grid" | "grid-area" | "grid-column" | "grid-column-end" | "grid-column-start"
        | "grid-row" | "grid-row-end" | "grid-row-start" => Some(GridPropertyKind::Generic),
        "grid-template" | "grid-template-columns" | "grid-template-rows" => {
            Some(GridPropertyKind::TemplateLike)
        },
        "grid-template-areas" => Some(GridPropertyKind::TemplateAreas),
        _ => None,
    })
}

fn is_reserved_grid_ident(ident: &str) -> bool {
    match_ignore_ascii_case!(ident, {
        "auto"
            | "span"
            | "auto-flow"
            | "dense"
            | "row"
            | "column"
            | "none"
            | "subgrid"
            | "masonry"
            | "max-content"
            | "min-content"
            | "initial"
            | "inherit"
            | "unset"
            | "revert"
            | "revert-layer" => true,
        _ => false,
    })
}

fn is_reserved_container_query_ident(ident: &str) -> bool {
    match_ignore_ascii_case!(ident, {
        "none" | "and" | "or" | "not" => true,
        _ => false,
    })
}

fn parse_grid_template_area_ranges(input: &str, offset: Pos) -> impl Iterator<Item = Range> + '_ {
    let bytes = input.as_bytes();
    let mut i = 0usize;

    std::iter::from_fn(move || loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if start == i {
            return None;
        }
        if !input[start..i].bytes().all(|c| c == b'.') {
            return Some(Range::new(offset + start as Pos, offset + i as Pos));
        }
    })
}

#[derive(Debug)]
pub struct LexDependencies<'s, W> {
    dependency_context: DependencyContext<'s>,
    mode_data: Option<ModeData<'s>>,
    scope: Scope<'s>,
    block_nesting_level: u32,
    allow_import_at_rule: bool,
    balanced: BalancedStack,
    is_next_rule_prelude: bool,
    scan_context: ScanContext,
    selector_square_depth: u32,
    selector_fast_forward_enabled: bool,
    property_kind: PropertyKind,
    in_animation_property: Option<InProperty<AnimationReserved>>,
    in_list_style_property: Option<InProperty<ListStyleReserved>>,
    in_font_palette_property: Option<InProperty<FontPaletteReserved>>,
    in_container_property: Option<InProperty<ContainerReserved>>,
    in_grid_property: Option<GridPropertyKind>,
    icss_symbols: FxHashSet<Cow<'s, str>>,
    icss_symbol_filter: [u64; 16],
    icss_symbol_min_len: usize,
    icss_symbol_max_len: usize,
    pending_custom_property: Option<(Range, TokenFlags)>,
    pending_grid_property: Option<GridPropertyKind>,
    handle_warning: W,
}

impl<'s, W: HandleWarning<'s>> LexDependencies<'s, W> {
    pub fn new(handle_warning: W, mode: Mode) -> Self {
        Self::with_context(DependencyContext::new(), handle_warning, mode)
    }

    pub(crate) fn with_context(
        dependency_context: DependencyContext<'s>,
        handle_warning: W,
        mode: Mode,
    ) -> Self {
        Self {
            dependency_context,
            mode_data: if mode == Mode::Css {
                None
            } else {
                Some(ModeData::new(mode))
            },
            scope: Scope::TopLevel,
            block_nesting_level: 0,
            allow_import_at_rule: true,
            balanced: Default::default(),
            is_next_rule_prelude: true,
            scan_context: ScanContext::TopLevel,
            selector_square_depth: 0,
            selector_fast_forward_enabled: false,
            property_kind: PropertyKind::Generic,
            in_animation_property: None,
            in_list_style_property: None,
            in_font_palette_property: None,
            in_container_property: None,
            in_grid_property: None,
            icss_symbols: Default::default(),
            icss_symbol_filter: [0; 16],
            icss_symbol_min_len: usize::MAX,
            icss_symbol_max_len: 0,
            pending_custom_property: None,
            pending_grid_property: None,
            handle_warning,
        }
    }

    pub fn dependency_context(&self) -> &DependencyContext<'s> {
        &self.dependency_context
    }

    pub fn into_dependency_context(self) -> DependencyContext<'s> {
        self.dependency_context
    }

    /// Drive dependency extraction from the forward token stream.
    pub fn lex_streaming(&mut self, source: &mut Lexer<'s>) {
        self.selector_fast_forward_enabled = source.source_end() >= 256;
        let mode = self
            .mode_data
            .as_ref()
            .map_or(Mode::Css, ModeData::default_mode);
        self.dependency_context
            .reserve_estimated_capacity(source.source_end() as usize, mode);
        let mut stream = TokenStream::from_lexer(source);
        let keep_comments = self.mode_data.as_ref().is_some_and(ModeData::is_pure_mode);
        let has_mode = self.mode_data.is_some();
        match (keep_comments, has_mode) {
            (true, true) => self.lex_streaming_inner::<true, true>(&mut stream),
            (true, false) => self.lex_streaming_inner::<true, false>(&mut stream),
            (false, true) => self.lex_streaming_inner::<false, true>(&mut stream),
            (false, false) => self.lex_streaming_inner::<false, false>(&mut stream),
        }
    }

    #[inline(always)]
    fn lex_streaming_inner<const KEEP_COMMENTS: bool, const HAS_MODE: bool>(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
    ) {
        loop {
            let item = stream.next::<KEEP_COMMENTS>();
            let token = item.token;
            if token.kind == TokenKind::Eof {
                return;
            }

            let is_trivia = matches!(token.kind, TokenKind::Comment | TokenKind::BadComment);
            if !is_trivia && self.scan_context == ScanContext::BlockItem {
                self.is_next_rule_prelude = !matches!(
                    token.kind,
                    TokenKind::Ident | TokenKind::Function | TokenKind::RightCurlyBracket
                );
                if self.is_next_rule_prelude && self.block_nesting_level == 0 {
                    if let Some(mode_data) = &mut self.mode_data {
                        mode_data.composes_local_classes.reset_to_initial();
                    }
                }
                self.scan_context = if self.is_next_rule_prelude {
                    ScanContext::Selector
                } else {
                    ScanContext::DeclarationName
                };
            }

            if self.scan_context == ScanContext::TopLevel
                && self.is_next_rule_prelude
                && token.kind != TokenKind::AtKeyword
            {
                self.scan_context = ScanContext::Selector;
            }

            let mut colon_next = None;
            let mut dot_next = None;
            if HAS_MODE
                && self.scan_context == ScanContext::Selector
                && token.kind == TokenKind::Colon
            {
                let first = stream.peek_significant_skipping_comments::<KEEP_COMMENTS>();
                if first.token.kind != TokenKind::Eof {
                    colon_next = Some(first);
                }
            } else if HAS_MODE
                && self.scan_context == ScanContext::Selector
                && token.kind == TokenKind::Delim
                && stream.lexer().byte_at(token.range.start) == Some(b'.')
            {
                let next = stream.peek_significant_skipping_comments::<KEEP_COMMENTS>();
                if next.token.kind != TokenKind::Eof {
                    dot_next = Some(next);
                }
            }

            let mut result = Some(());
            match token.kind {
                TokenKind::Comment | TokenKind::BadComment => {
                    result =
                        self.handle_comment(stream.lexer_mut(), token.range.start, token.range.end);
                }
                TokenKind::WhiteSpace => {}
                TokenKind::QuotedString | TokenKind::BadString => {
                    result = self.handle_string(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Url => {
                    result = self.handle_url(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                        token.value_range.start,
                        token.value_range.end,
                        token.flags,
                    );
                }
                TokenKind::Function => {
                    result = self.handle_function(
                        stream,
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Ident => {
                    result =
                        self.handle_ident(stream, token.range.start, token.range.end, token.flags);
                }
                TokenKind::AtKeyword => {
                    result = self.handle_at_keyword(
                        stream,
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::IdHash | TokenKind::Hash
                    if !HAS_MODE || self.scan_context != ScanContext::Selector => {}
                TokenKind::IdHash | TokenKind::Hash => {
                    let id_end = if token.kind == TokenKind::Hash {
                        token.range.start + 1
                    } else {
                        token.range.end
                    };
                    result =
                        self.handle_id(stream.lexer_mut(), token.range.start, id_end, token.flags);
                }
                TokenKind::Delim
                    if (!HAS_MODE || self.scan_context != ScanContext::Selector)
                        && stream.byte_at(token.range.start) == Some(b'#') => {}
                TokenKind::Delim if stream.byte_at(token.range.start) == Some(b'#') => {
                    result = self.handle_id(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Delim
                    if (!HAS_MODE || self.scan_context != ScanContext::Selector)
                        && stream.byte_at(token.range.start) == Some(b'.') => {}
                TokenKind::Delim if stream.byte_at(token.range.start) == Some(b'.') => {
                    let mut class_end = token.range.end;
                    let mut class_flags = token.flags;
                    let mut consumes_name = false;
                    if let Some(next) = dot_next {
                        if next.token.kind == TokenKind::Ident
                            && next.token.range.start == token.range.end
                        {
                            class_end = next.token.range.end;
                            class_flags = next.token.flags;
                            consumes_name = true;
                        }
                    }
                    result = self.handle_class(
                        stream.lexer_mut(),
                        token.range.start,
                        class_end,
                        class_flags,
                    );
                    if consumes_name {
                        stream.next::<KEEP_COMMENTS>();
                    }
                }
                TokenKind::Delim if self.scan_context == ScanContext::Selector => {
                    if self.block_nesting_level == 0 {
                        if let Some(mode_data) = &mut self.mode_data {
                            mode_data.composes_local_classes.invalidate();
                        }
                    }
                }
                TokenKind::Colon if self.scan_context == ScanContext::DeclarationName => {
                    result = self.enter_property_value(stream.lexer());
                }
                TokenKind::Colon if !HAS_MODE => {}
                TokenKind::Colon => {
                    if let Some(next) = colon_next.filter(|next| {
                        matches!(next.token.kind, TokenKind::Ident | TokenKind::Function)
                            && next.token.range.start == token.range.end
                    }) {
                        let (end, function) =
                            (next.token.range.end, next.token.kind == TokenKind::Function);
                        stream.next::<KEEP_COMMENTS>();
                        result = if function {
                            self.handle_pseudo_function(
                                stream,
                                token.range.start,
                                end,
                                next.token.flags,
                            )
                        } else {
                            self.handle_pseudo_class(
                                stream,
                                token.range.start,
                                end,
                                next.token.flags,
                            )
                        };
                    }
                }
                TokenKind::LeftSquareBracket if self.scan_context == ScanContext::Selector => {
                    self.selector_square_depth += 1;
                    if self.block_nesting_level == 0 {
                        if let Some(mode_data) = &mut self.mode_data {
                            mode_data.composes_local_classes.invalidate();
                        }
                    }
                }
                TokenKind::RightSquareBracket if self.scan_context == ScanContext::Selector => {
                    self.selector_square_depth = self.selector_square_depth.saturating_sub(1);
                }
                TokenKind::LeftParenthesis => {
                    result = self.handle_left_parenthesis(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::RightParenthesis => {
                    result = self.handle_right_parenthesis(
                        stream.lexer_mut(),
                        item.leading.range.start,
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::Comma => {
                    result =
                        self.handle_comma(stream.lexer_mut(), token.range.start, token.range.end);
                }
                TokenKind::Semicolon => {
                    result = self.handle_semicolon(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::LeftCurlyBracket => {
                    result = self.handle_left_curly_bracket(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::RightCurlyBracket => {
                    result = self.handle_right_curly_bracket(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                _ => {}
            }

            if result.is_none() {
                return;
            }
            if token.kind == TokenKind::Semicolon && matches!(self.scope, Scope::TopLevel) {
                self.scan_context = ScanContext::TopLevel;
                self.is_next_rule_prelude = true;
            }
            if self.scan_context != ScanContext::Selector {
                self.selector_square_depth = 0;
            }
            match self.scan_context {
                ScanContext::Selector if self.selector_fast_forward_enabled => {
                    self.fast_forward_selector::<KEEP_COMMENTS, HAS_MODE>(stream);
                }
                ScanContext::AtRule
                    if !matches!(self.scope, Scope::InAtImport(_) | Scope::AtImportInvalid) =>
                {
                    self.fast_forward_at_rule::<KEEP_COMMENTS>(stream);
                }
                ScanContext::SpecialValue(property) => {
                    self.fast_forward_special_value::<KEEP_COMMENTS>(stream, property);
                }
                ScanContext::GenericValue if !is_trivia => {
                    self.fast_forward_generic_value::<KEEP_COMMENTS>(stream);
                }
                _ => {}
            }
            if self.scan_context != ScanContext::AtRule {
                stream.reset_at_rule_scan_state();
            }
            if !matches!(self.scan_context, ScanContext::SpecialValue(_)) {
                stream.reset_special_value_scan_state();
            }
        }
    }

    #[inline]
    fn fast_forward_selector<const KEEP_COMMENTS: bool, const HAS_MODE: bool>(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
    ) {
        let invalidates_composes = stream.fast_forward_selector_if_buffer_empty(
            &mut self.selector_square_depth,
            KEEP_COMMENTS,
            HAS_MODE,
            is_css_modules_pure_magic_comment,
        );
        if invalidates_composes && self.block_nesting_level == 0 {
            if let Some(mode_data) = &mut self.mode_data {
                mode_data.composes_local_classes.invalidate();
            }
        }
    }

    #[inline]
    fn fast_forward_generic_value<const KEEP_COMMENTS: bool>(
        &self,
        stream: &mut TokenStream<'_, 's>,
    ) {
        let preserve_strings = matches!(
            self.balanced.last(),
            Some(last) if matches!(last.kind, BalancedItemKind::Url | BalancedItemKind::ImageSet)
        );
        let preserve_delimiters = !self.balanced.is_empty();
        stream.fast_forward_if_buffer_empty(
            KEEP_COMMENTS,
            preserve_strings,
            preserve_delimiters,
            |ident| self.contains_icss_symbol(ident),
            is_css_modules_pure_magic_comment,
        );
    }

    #[inline]
    fn fast_forward_at_rule<const KEEP_COMMENTS: bool>(&self, stream: &mut TokenStream<'_, 's>) {
        let preserve_strings = matches!(
            self.balanced.last(),
            Some(last) if matches!(last.kind, BalancedItemKind::Url | BalancedItemKind::ImageSet)
        );
        let preserve_delimiters = !self.balanced.is_empty();
        stream.fast_forward_at_rule_if_buffer_empty(
            KEEP_COMMENTS,
            preserve_strings,
            preserve_delimiters,
            |ident| self.contains_icss_symbol(ident),
            is_css_modules_pure_magic_comment,
        );
    }

    #[inline]
    fn fast_forward_special_value<const KEEP_COMMENTS: bool>(
        &self,
        stream: &mut TokenStream<'_, 's>,
        property: PropertyKind,
    ) {
        // An animation name may be any top-level identifier, so the raw
        // scanner usually stops immediately. Continue tokenizing directly.
        if property == PropertyKind::Animation {
            return;
        }
        let preserve_strings = property == PropertyKind::Grid
            || matches!(
                self.balanced.last(),
                Some(last) if matches!(last.kind, BalancedItemKind::Url | BalancedItemKind::ImageSet)
            );
        let preserve_delimiters = !self.balanced.is_empty();
        stream.fast_forward_special_value_if_buffer_empty(
            KEEP_COMMENTS,
            preserve_strings,
            preserve_delimiters,
            |ident| {
                if self.contains_icss_symbol(ident) {
                    return true;
                }
                match property {
                    PropertyKind::ListStyle => ListStyleReserved.check(ident),
                    PropertyKind::FontPalette => ident.starts_with("--"),
                    PropertyKind::Container => ContainerReserved.check(ident),
                    PropertyKind::Grid => !is_reserved_grid_ident(ident),
                    PropertyKind::Animation
                    | PropertyKind::Generic
                    | PropertyKind::Composes
                    | PropertyKind::CustomProperty => false,
                }
            },
            is_css_modules_pure_magic_comment,
        );
    }

    #[inline]
    fn icss_symbol_filter_bit(value: &str) -> (usize, u64) {
        let bytes = value.as_bytes();
        let len = bytes.len();
        let first = bytes.first().copied().unwrap_or_default() as usize;
        let middle = bytes.get(len / 2).copied().unwrap_or_default() as usize;
        let last = bytes.last().copied().unwrap_or_default() as usize;
        let mut hash = len.wrapping_mul(0x9e37_79b1);
        hash ^= first.wrapping_mul(0x85eb_ca6b);
        hash ^= middle.wrapping_mul(0xc2b2_ae35);
        hash ^= last.wrapping_mul(0x27d4_eb2f);
        hash ^= hash >> 16;
        let bit = hash & 1023;
        (bit >> 6, 1u64 << (bit & 63))
    }

    #[inline]
    fn contains_icss_symbol(&self, value: &str) -> bool {
        let len = value.len();
        if len < self.icss_symbol_min_len || len > self.icss_symbol_max_len {
            return false;
        }
        let (word, bit) = Self::icss_symbol_filter_bit(value);
        self.icss_symbol_filter[word] & bit != 0 && self.icss_symbols.contains(value)
    }

    fn insert_icss_symbol(&mut self, value: Cow<'s, str>) {
        let len = value.len();
        self.icss_symbol_min_len = self.icss_symbol_min_len.min(len);
        self.icss_symbol_max_len = self.icss_symbol_max_len.max(len);
        let (word, bit) = Self::icss_symbol_filter_bit(value.as_ref());
        self.icss_symbol_filter[word] |= bit;
        self.icss_symbols.insert(value);
    }

    fn enter_property_value(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        match self.property_kind {
            PropertyKind::Animation => self.enter_animation_property(),
            PropertyKind::ListStyle => self.enter_list_style_property(),
            PropertyKind::FontPalette => self.enter_font_palette_property(),
            PropertyKind::Container => self.enter_container_property(),
            PropertyKind::Grid => {
                if let Some(kind) = self.pending_grid_property.take() {
                    self.enter_grid_property(kind);
                }
            }
            PropertyKind::CustomProperty => {
                if self
                    .mode_data
                    .as_ref()
                    .is_some_and(ModeData::is_property_local_mode)
                {
                    if let Some((range, flags)) = self.pending_custom_property.take() {
                        self.dependency_context
                            .push_dependency(Dependency::LocalVarDecl {
                                name: unescape_token(
                                    lexer.slice(range.start + 2, range.end)?,
                                    flags,
                                ),
                                range,
                            });
                    }
                }
            }
            PropertyKind::Generic | PropertyKind::Composes => {}
        }
        self.pending_custom_property = None;
        self.pending_grid_property = None;
        self.scan_context = ScanContext::for_property(self.property_kind);
        Some(())
    }

    fn classify_property(
        name: &str,
        property_local_mode: bool,
    ) -> (PropertyKind, Option<GridPropertyKind>) {
        if name.eq_ignore_ascii_case("composes") || name.eq_ignore_ascii_case("compose-with") {
            return (PropertyKind::Composes, None);
        }
        if !property_local_mode {
            return (PropertyKind::Generic, None);
        }
        if name.starts_with("--") {
            return (PropertyKind::CustomProperty, None);
        }
        if name.eq_ignore_ascii_case("animation")
            || name.eq_ignore_ascii_case("animation-name")
            || with_vendor_prefixed_eq(name, "animation", false)
            || with_vendor_prefixed_eq(name, "animation-name", false)
        {
            return (PropertyKind::Animation, None);
        }
        if name.eq_ignore_ascii_case("list-style") || name.eq_ignore_ascii_case("list-style-type") {
            return (PropertyKind::ListStyle, None);
        }
        if name.eq_ignore_ascii_case("font-palette") {
            return (PropertyKind::FontPalette, None);
        }
        if name.eq_ignore_ascii_case("container") || name.eq_ignore_ascii_case("container-name") {
            return (PropertyKind::Container, None);
        }
        if let Some(grid) = grid_property_kind(name) {
            return (PropertyKind::Grid, Some(grid));
        }
        (PropertyKind::Generic, None)
    }

    fn classify_at_rule(name: &str) -> AtRuleKind {
        match name.len() {
            6 if name.eq_ignore_ascii_case("@value") => AtRuleKind::Value,
            6 if name.eq_ignore_ascii_case("@scope") => AtRuleKind::Scope,
            7 if name.eq_ignore_ascii_case("@import") => AtRuleKind::Import,
            8 if name.eq_ignore_ascii_case("@charset") => AtRuleKind::Charset,
            9 if name.eq_ignore_ascii_case("@function") => AtRuleKind::Function,
            9 if name.eq_ignore_ascii_case("@property") => AtRuleKind::Property,
            10 if name.eq_ignore_ascii_case("@namespace") => AtRuleKind::Namespace,
            10 if name.eq_ignore_ascii_case("@keyframes") => AtRuleKind::Keyframes,
            10 if name.eq_ignore_ascii_case("@container") => AtRuleKind::Container,
            14 if name.eq_ignore_ascii_case("@counter-style") => AtRuleKind::CounterStyle,
            20 if name.eq_ignore_ascii_case("@font-palette-values") => {
                AtRuleKind::FontPaletteValues
            }
            _ if with_vendor_prefixed_eq(name, "keyframes", true) => AtRuleKind::Keyframes,
            _ => AtRuleKind::Other,
        }
    }

    fn get_media(&self, lexer: &Lexer<'s>, start: Pos, end: Pos) -> Option<&'s str> {
        let media = lexer.slice(start, end)?;
        let bytes = media.as_bytes();
        let mut position = 0;
        loop {
            while position < bytes.len() && bytes[position].is_ascii_whitespace() {
                position += 1;
            }
            if position + 1 < bytes.len() && bytes[position] == b'/' && bytes[position + 1] == b'*'
            {
                let Some(relative_end) = media[position + 2..].find("*/") else {
                    break;
                };
                position += relative_end + 4;
                continue;
            }
            break;
        }
        if position == bytes.len() {
            return None;
        }
        Some(media)
    }

    fn lex_charset_at_rule(&mut self, stream: &mut TokenStream<'_, 's>, start: Pos) -> Option<()> {
        let string = stream.next_parser_token().token;
        if string.kind != TokenKind::QuotedString {
            return Some(());
        }

        let next = stream.next_parser_token().token;
        if next.kind == TokenKind::Semicolon {
            self.dependency_context
                .push_dependency(Dependency::Charset {
                    value: unescape_token(
                        stream.slice(string.value_range.start, string.value_range.end)?,
                        string.flags,
                    ),
                    range: Range::new(start, next.range.end),
                });
        }
        Some(())
    }

    fn enter_animation_property(&mut self) {
        self.in_animation_property = Some(InProperty::new(
            AnimationReserved::default(),
            self.balanced.len(),
        ));
    }

    fn exit_animation_property(&mut self) {
        self.in_animation_property = None;
    }

    fn enter_list_style_property(&mut self) {
        self.in_list_style_property = Some(InProperty::new(ListStyleReserved, self.balanced.len()));
    }

    fn exit_list_style_property(&mut self) {
        self.in_list_style_property = None;
    }

    fn enter_font_palette_property(&mut self) {
        self.in_font_palette_property =
            Some(InProperty::new(FontPaletteReserved, self.balanced.len()));
    }

    fn exit_font_palette_property(&mut self) {
        self.in_font_palette_property = None;
    }

    fn enter_container_property(&mut self) {
        self.in_container_property = Some(InProperty::new(ContainerReserved, self.balanced.len()));
    }

    fn exit_container_property(&mut self) {
        self.in_container_property = None;
    }

    fn enter_grid_property(&mut self, kind: GridPropertyKind) {
        self.in_grid_property = Some(kind);
    }

    fn exit_grid_property(&mut self) {
        self.in_grid_property = None;
    }

    fn lex_icss_import(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let (start, end) = self.consume_icss_import_path(stream)?;
        let right_parenthesis = stream.next_parser_token().token;
        if right_parenthesis.kind != TokenKind::RightParenthesis {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected ')' during parsing of ':import()'",
                },
            });
            return Some(());
        }
        self.dependency_context
            .push_dependency(Dependency::ICSSImportFrom {
                path: unescape(stream.slice(start, end)?),
            });
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of ':import()'",
                },
            });
            return Some(());
        }
        loop {
            let first = stream.next_parser_token().token;
            if first.kind == TokenKind::Eof {
                return None;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                break;
            }
            let prop_start = first.range.start;
            let prop_end = self.consume_icss_export_prop(stream, first)?;
            let colon = stream.peek_significant_skipping_comments::<true>().token;
            if colon.kind != TokenKind::Colon {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(colon.range.start, colon.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':' during parsing of ':import'",
                    },
                });
                return Some(());
            }
            stream.next_parser_token();
            let value_start_token = stream.next_parser_token().token;
            if value_start_token.kind == TokenKind::Eof {
                return None;
            }
            let value_start = value_start_token.range.start;
            let value_end = self.consume_icss_value(stream, value_start_token, |_| {})?;
            let delimiter = stream.next_parser_token().token;
            self.dependency_context
                .push_dependency(Dependency::ICSSImportValue {
                    prop: unescape(
                        stream
                            .slice(prop_start, prop_end)?
                            .trim_end_matches(is_css_white_space_char),
                    ),
                    value: unescape(
                        stream
                            .slice(value_start, value_end)?
                            .trim_end_matches(is_css_white_space_char),
                    ),
                });
            self.insert_icss_symbol(unescape(
                stream
                    .slice(prop_start, prop_end)?
                    .trim_end_matches(is_css_white_space_char),
            ));
            if delimiter.kind == TokenKind::RightCurlyBracket {
                break;
            }
        }
        Some(())
    }

    fn consume_icss_import_path(&self, stream: &mut TokenStream<'_, 's>) -> Option<(Pos, Pos)> {
        let first = stream.next_parser_token().token;
        if first.kind == TokenKind::Eof {
            return None;
        }
        let start = first.range.start;
        let mut end = start;
        if first.kind == TokenKind::RightParenthesis {
            return Some((start, end));
        }
        end = first.range.end;
        loop {
            let token = stream.peek_significant_skipping_comments::<true>().token;
            if token.kind == TokenKind::Eof {
                return None;
            }
            if token.kind == TokenKind::RightParenthesis {
                return Some((start, end));
            }
            stream.next_parser_token();
            if first.kind != TokenKind::QuotedString {
                end = token.range.end;
            }
        }
    }

    fn consume_icss_export_prop(
        &self,
        stream: &mut TokenStream<'_, 's>,
        first: Token,
    ) -> Option<Pos> {
        if matches!(
            first.kind,
            TokenKind::Colon | TokenKind::RightCurlyBracket | TokenKind::Semicolon
        ) {
            return Some(first.range.start);
        }
        loop {
            let token = stream.peek_parser_token();
            if token.token.kind == TokenKind::Eof {
                return None;
            }
            if token.leading.first_comment_start.is_some() {
                return Some(token.leading.first_comment_start.unwrap());
            }
            if matches!(
                token.token.kind,
                TokenKind::Colon | TokenKind::RightCurlyBracket | TokenKind::Semicolon
            ) {
                return Some(token.token.range.start);
            }
            stream.next_parser_token();
        }
    }

    fn consume_icss_value(
        &self,
        stream: &mut TokenStream<'_, 's>,
        first: Token,
        mut handle_comment: impl FnMut(Range),
    ) -> Option<Pos> {
        if matches!(
            first.kind,
            TokenKind::RightCurlyBracket | TokenKind::Semicolon
        ) {
            return Some(first.range.start);
        }
        loop {
            let token = stream.peek::<true>().token;
            if token.kind == TokenKind::Eof {
                return None;
            }
            if matches!(
                token.kind,
                TokenKind::RightCurlyBracket | TokenKind::Semicolon
            ) {
                return Some(token.range.start);
            }
            let token = stream.next::<true>().token;
            if matches!(token.kind, TokenKind::Comment | TokenKind::BadComment) {
                handle_comment(token.range);
            }
        }
    }

    fn consume_value_at_rule_params(
        &self,
        stream: &mut TokenStream<'_, 's>,
    ) -> Option<(Pos, Pos, SmallVec<[Token; 16]>)> {
        let start = stream.consumed_pos();
        let mut tokens = SmallVec::new();
        let mut depth = 0u32;
        loop {
            let token = stream.peek_significant_skipping_comments::<true>().token;
            if token.kind == TokenKind::Eof {
                return None;
            }
            if is_close_token(token.kind) {
                depth = depth.saturating_sub(1);
            }
            if depth == 0 && token.kind == TokenKind::Semicolon {
                stream.next_parser_token();
                return Some((start, token.range.start, tokens));
            }
            if depth == 0 && token.kind == TokenKind::RightCurlyBracket {
                return Some((start, token.range.start, tokens));
            }
            let token = stream.next_parser_token().token;
            tokens.push(Token::with_flags(
                token.kind,
                Range::new(token.range.start - start, token.range.end - start),
                Range::new(
                    token.value_range.start - start,
                    token.value_range.end - start,
                ),
                token.flags,
            ));
            if is_open_token(token.kind) {
                depth += 1;
            }
        }
    }

    fn lex_value_at_rule(&mut self, stream: &mut TokenStream<'_, 's>, start: Pos) -> Option<()> {
        let (params_start, params_end, tokens) = self.consume_value_at_rule_params(stream)?;
        let at_rule_end = params_end.max(stream.consumed_pos());
        let params = stream.slice(params_start, params_end)?;
        match parse_value_at_rule(params, &tokens, &mut self.dependency_context) {
            ValueAtRule::Import { from, items } => {
                if from.is_empty() || items.is_empty() {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, at_rule_end),
                        kind: WarningKind::Unexpected {
                            message: "Broken '@value' at-rule",
                        },
                    });
                } else {
                    self.dependency_context
                        .push_dependency(Dependency::ICSSImportFrom {
                            path: unescape(from),
                        });
                    for index in items.as_usize_range() {
                        let item = self.dependency_context.value_at_rule_import_item(index);
                        self.dependency_context
                            .push_dependency(Dependency::ICSSImportValue {
                                prop: unescape(item.local_name()),
                                value: unescape(item.import_name()),
                            });
                        self.insert_icss_symbol(unescape(item.local_name()));
                        self.dependency_context
                            .push_dependency(Dependency::ICSSExportValue {
                                prop: unescape(item.local_name()),
                                value: unescape(item.local_name()),
                            });
                    }
                }
            }
            ValueAtRule::Value {
                local_name,
                value,
                has_colon,
            } => {
                if local_name.is_empty() || (!has_colon && value.is_empty()) {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, at_rule_end),
                        kind: WarningKind::Unexpected {
                            message: "Broken '@value' at-rule",
                        },
                    });
                }
                if !local_name.is_empty() {
                    self.dependency_context
                        .push_dependency(Dependency::ICSSExportValue {
                            prop: unescape(local_name),
                            value: Cow::Borrowed(value),
                        });
                    self.insert_icss_symbol(unescape(local_name));
                }
            }
        }
        self.dependency_context
            .push_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, at_rule_end),
            });
        Some(())
    }

    fn lex_icss_export(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of ':export'",
                },
            });
            return Some(());
        }
        loop {
            let first = stream.next_parser_token().token;
            if first.kind == TokenKind::Eof {
                return None;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                break;
            }
            let prop_start = first.range.start;
            let prop_end = self.consume_icss_export_prop(stream, first)?;
            let colon = stream.peek_significant_skipping_comments::<true>().token;
            if colon.kind != TokenKind::Colon {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(colon.range.start, colon.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':' during parsing of ':export'",
                    },
                });
                return Some(());
            }
            stream.next_parser_token();
            let value_start_token = stream.next_parser_token().token;
            if value_start_token.kind == TokenKind::Eof {
                return None;
            }
            let value_start = value_start_token.range.start;
            let input = stream.slice(0, stream.source_end())?;
            let mut value_without_comments = None;
            let mut copied_until = value_start;
            let value_end = self.consume_icss_value(stream, value_start_token, |comment| {
                let output = value_without_comments.get_or_insert_with(String::new);
                output.push_str(&input[copied_until as usize..comment.start as usize]);
                copied_until = comment.end;
            })?;
            let delimiter = stream.next_parser_token().token;
            let value = stream
                .slice(value_start, value_end)?
                .trim_end_matches(is_css_white_space_char);
            let trimmed_value_end = value_start + value.len() as Pos;
            let value = if let Some(mut output) = value_without_comments {
                output.push_str(&input[copied_until as usize..trimmed_value_end as usize]);
                Cow::Owned(output)
            } else {
                Cow::Borrowed(value)
            };
            self.dependency_context
                .push_dependency(Dependency::ICSSExportValue {
                    prop: unescape(
                        stream
                            .slice(prop_start, prop_end)?
                            .trim_end_matches(is_css_white_space_char),
                    ),
                    value,
                });
            self.insert_icss_symbol(unescape(
                stream
                    .slice(prop_start, prop_end)?
                    .trim_end_matches(is_css_white_space_char),
            ));
            if delimiter.kind == TokenKind::RightCurlyBracket {
                break;
            }
        }
        Some(())
    }

    fn lex_local_var(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let name_token = stream.next_parser_token().token;
        let start = name_token.range.start;
        if name_token.kind != TokenKind::Ident
            || !stream.slice(start, name_token.range.end)?.starts_with("--")
        {
            self.handle_warning.handle_warning(Warning {
                kind: WarningKind::Unexpected {
                    message: "Expected starts with '--' during parsing of 'var()'",
                },
                range: Range::new(start, (start + 2).min(name_token.range.end)),
            });
            return Some(());
        }
        let end = name_token.range.end;
        let next = stream.peek_significant_skipping_comments::<true>().token;
        let from = if next.kind == TokenKind::Ident
            && stream.slice(next.range.start, next.range.end)? == "from"
        {
            stream.next_parser_token();
            let path = stream.peek_significant_skipping_comments::<true>().token;
            let path_start = path.range.start;
            let path_end = path.range.end;
            if !matches!(path.kind, TokenKind::QuotedString | TokenKind::Ident) {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(path_start, path_end),
                    kind: WarningKind::Unexpected {
                        message: "Expected string or ident during parsing of 'composes'",
                    },
                });
                return Some(());
            }
            stream.next_parser_token();
            Some(unescape_token(
                stream.slice(path_start, path_end)?,
                path.flags,
            ))
        } else {
            None
        };
        self.dependency_context
            .push_dependency(Dependency::LocalVar {
                name: unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                range: Range::new(start, end),
                from,
            });
        Some(())
    }

    fn lex_local_dashed_ident_decl(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        local_decl_dependency: impl FnOnce(Cow<'s, str>, Range) -> Dependency<'s>,
        dashed_warning: impl FnOnce(Range) -> Warning<'s>,
        left_curly_warning: impl FnOnce(Range) -> Warning<'s>,
    ) -> Option<()> {
        let name_token = stream.next_parser_token().token;
        let start = name_token.range.start;
        if name_token.kind != TokenKind::Ident
            || !stream.slice(start, name_token.range.end)?.starts_with("--")
        {
            self.handle_warning
                .handle_warning(dashed_warning(Range::new(
                    start,
                    (start + 2).min(name_token.range.end),
                )));
            return Some(());
        }
        let end = name_token.range.end;
        self.dependency_context
            .push_dependency(local_decl_dependency(
                unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                Range::new(start, end),
            ));
        let left_curly = stream.peek_significant_skipping_comments::<true>().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning
                .handle_warning(left_curly_warning(Range::new(
                    left_curly.range.start,
                    left_curly.range.end,
                )));
            return Some(());
        }
        Some(())
    }

    fn lex_local_keyframes_decl(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let mut is_function = false;
        let first = stream.next_parser_token().token;
        let name_token = if first.kind == TokenKind::Colon {
            let pseudo_start = first.range.start;
            let pseudo_name = stream.next_parser_token().token;
            let pseudo_end = if matches!(pseudo_name.kind, TokenKind::Ident | TokenKind::Function) {
                pseudo_name.range.end
            } else {
                first.range.end
            };
            let pseudo = stream.slice(pseudo_start, pseudo_end)?;
            if pseudo_name.kind == TokenKind::Function {
                self.handle_pseudo_function(stream, pseudo_start, pseudo_end, pseudo_name.flags)?;
            } else if pseudo_name.kind == TokenKind::Ident {
                self.handle_pseudo_class(stream, pseudo_start, pseudo_end, pseudo_name.flags)?;
            }
            let mode_data = self.mode_data.as_ref().unwrap();
            if mode_data.is_pure_mode()
                && !mode_data.is_pure_check_disabled()
                && (pseudo.eq_ignore_ascii_case(":global(")
                    || pseudo.eq_ignore_ascii_case(":global"))
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(pseudo_start, pseudo_end),
                    kind: WarningKind::NotPure {
                        message: "'@keyframes :global' is not allowed in pure mode",
                    },
                });
            }
            is_function =
                pseudo.eq_ignore_ascii_case(":local(") || pseudo.eq_ignore_ascii_case(":global(");
            if !is_function
                && !pseudo.eq_ignore_ascii_case(":local")
                && !pseudo.eq_ignore_ascii_case(":global")
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(pseudo_start, pseudo_end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':local', ':local()', ':global', or ':global()' during parsing of '@keyframes' name",
                    }
                });
                return Some(());
            }
            stream.next_parser_token().token
        } else {
            first
        };

        let start = name_token.range.start;
        if name_token.kind != TokenKind::Ident {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, start.saturating_add(2).min(stream.source_end())),
                kind: WarningKind::Unexpected {
                    message: "Expected ident during parsing of '@keyframes' name",
                },
            });
            return Some(());
        }
        let end = name_token.range.end;
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            let name = stream.slice(start, end)?;
            self.dependency_context
                .push_dependency(Dependency::LocalKeyframesDecl {
                    name: unescape_token(name, name_token.flags),
                    range: Range::new(start, end),
                });
        }
        if is_function {
            let right_parenthesis = stream.peek_significant_skipping_comments::<true>().token;
            if right_parenthesis.kind != TokenKind::RightParenthesis {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ')' during parsing of '@keyframes :local(' or '@keyframes :global('",
                    },
                });
                return Some(());
            }
            stream.next_parser_token();
            self.dependency_context
                .push_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                });
            let mode_data = self.mode_data.as_mut().unwrap();
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
        }
        let left_curly = stream.peek_significant_skipping_comments::<true>().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@keyframes'",
                },
            });
            return Some(());
        }
        Some(())
    }

    fn handle_local_keyframes_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let animation = self.in_animation_property.as_mut().unwrap();
        if let Some((range, flags)) = animation.take_rename(self.balanced.len()) {
            self.dependency_context
                .push_dependency(Dependency::LocalKeyframes {
                    name: unescape_token(lexer.slice(range.start, range.end)?, flags),
                    range,
                });
        }
        animation.reset_reserved();
        Some(())
    }

    fn lex_local_counter_style_decl(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let name_token = stream.next_parser_token().token;
        let start = name_token.range.start;
        if name_token.kind != TokenKind::Ident {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, name_token.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected ident during parsing of '@counter-style'",
                },
            });
            return Some(());
        }
        let end = name_token.range.end;
        self.dependency_context
            .push_dependency(Dependency::LocalCounterStyleDecl {
                name: unescape_token(stream.slice(start, end)?, name_token.flags),
                range: Range::new(start, end),
            });
        let left_curly = stream.peek_significant_skipping_comments::<true>().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@counter-style'",
                },
            });
            return Some(());
        }
        Some(())
    }

    fn lex_local_container_at_rule(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let name_token = stream.next_parser_token().token;
        if name_token.kind == TokenKind::LeftParenthesis {
            return Some(());
        }
        if name_token.kind != TokenKind::Ident {
            return Some(());
        }
        let start = name_token.range.start;
        let end = name_token.range.end;
        let ident = stream.slice(start, end)?;
        if is_reserved_container_query_ident(ident) {
            return Some(());
        }
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            self.dependency_context
                .push_dependency(Dependency::LocalContainer {
                    name: unescape_token(ident, name_token.flags),
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn lex_local_function_decl(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let mut is_function = false;
        let first = stream.next_parser_token().token;
        let name_token = if first.kind == TokenKind::Colon {
            let pseudo_start = first.range.start;
            let pseudo_name = stream.next_parser_token().token;
            let pseudo_end = if matches!(pseudo_name.kind, TokenKind::Ident | TokenKind::Function) {
                pseudo_name.range.end
            } else {
                first.range.end
            };
            let pseudo = stream.slice(pseudo_start, pseudo_end)?;
            if pseudo_name.kind == TokenKind::Function {
                self.handle_pseudo_function(stream, pseudo_start, pseudo_end, pseudo_name.flags)?;
            } else if pseudo_name.kind == TokenKind::Ident {
                self.handle_pseudo_class(stream, pseudo_start, pseudo_end, pseudo_name.flags)?;
            }
            let mode_data = self.mode_data.as_ref().unwrap();
            if mode_data.is_pure_mode()
                && !mode_data.is_pure_check_disabled()
                && (pseudo.eq_ignore_ascii_case(":global(")
                    || pseudo.eq_ignore_ascii_case(":global"))
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(pseudo_start, pseudo_end),
                    kind: WarningKind::NotPure {
                        message: "'@function :global' is not allowed in pure mode",
                    },
                });
            }
            is_function =
                pseudo.eq_ignore_ascii_case(":local(") || pseudo.eq_ignore_ascii_case(":global(");
            if !is_function
                && !pseudo.eq_ignore_ascii_case(":local")
                && !pseudo.eq_ignore_ascii_case(":global")
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(pseudo_start, pseudo_end),
                    kind: WarningKind::Unexpected {
                        message:
                            "Expected ':local', ':local()', ':global', or ':global()' during parsing of '@function' name",
                    },
                });
                return Some(());
            }
            stream.next_parser_token().token
        } else {
            first
        };

        let name_range = if name_token.kind == TokenKind::Function {
            name_token.value_range
        } else {
            name_token.range
        };
        let start = name_range.start;
        let end = name_range.end;
        let name = stream.slice_trusted(start, end);
        if name_token.kind != TokenKind::Ident && name_token.kind != TokenKind::Function
            || !name.starts_with("--")
        {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, start.saturating_add(2).min(stream.source_end())),
                kind: WarningKind::Unexpected {
                    message: "Expected starts with '--' during parsing of '@function' name",
                },
            });
            return Some(());
        }
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            self.dependency_context
                .push_dependency(Dependency::LocalFunctionDecl {
                    name: unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                    range: Range::new(start, end),
                });
        }

        if is_function {
            let right_parenthesis = stream.peek_significant_skipping_comments::<true>().token;
            if right_parenthesis.kind != TokenKind::RightParenthesis {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(
                        right_parenthesis.range.start,
                        right_parenthesis.range.end,
                    ),
                    kind: WarningKind::Unexpected {
                        message:
                            "Expected ')' during parsing of '@function :local(' or '@function :global('",
                    },
                });
                return Some(());
            }
            stream.next_parser_token();
            self.dependency_context
                .push_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                });
            let mode_data = self.mode_data.as_mut().unwrap();
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
        }

        if name_token.kind == TokenKind::Function {
            return Some(());
        }
        let left_parenthesis = stream.peek_significant_skipping_comments::<true>().token;
        if left_parenthesis.kind != TokenKind::LeftParenthesis {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_parenthesis.range.start, left_parenthesis.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '(' during parsing of '@function'",
                },
            });
        }
        Some(())
    }

    fn handle_local_counter_style_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let list_style = self.in_list_style_property.as_mut().unwrap();
        if let Some((range, flags)) = list_style.take_rename(self.balanced.len()) {
            self.dependency_context
                .push_dependency(Dependency::LocalCounterStyle {
                    name: unescape_token(lexer.slice(range.start, range.end)?, flags),
                    range,
                });
        }
        Some(())
    }

    fn handle_local_font_palette_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let font_palette = self.in_font_palette_property.as_mut().unwrap();
        if let Some((range, flags)) = font_palette.take_rename(self.balanced.len()) {
            self.dependency_context
                .push_dependency(Dependency::LocalFontPalette {
                    name: unescape_token(lexer.slice(range.start + 2, range.end)?, flags),
                    range,
                });
        }
        Some(())
    }

    fn lex_composes(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        local_classes: SmallVec<[&'s str; 2]>,
        start: Pos,
    ) -> Option<()> {
        let colon = stream.peek_significant_skipping_comments::<true>().token;
        if colon.kind != TokenKind::Colon {
            return Some(());
        }
        stream.next_parser_token();
        let mut replacement_end = colon.range.end;
        loop {
            let first = stream.peek_significant_skipping_comments::<true>().token;
            if first.kind == TokenKind::Eof {
                break;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                break;
            }
            if first.kind == TokenKind::Semicolon {
                stream.next_parser_token();
                replacement_end = first.range.end;
                break;
            }

            let item_start = first.range.start;
            let mut item_end = item_start;
            let mut names: SmallVec<[&'s str; 2]> = SmallVec::new();
            let mut has_from = false;
            let mut delimiter = first;
            loop {
                if matches!(
                    delimiter.kind,
                    TokenKind::Comma | TokenKind::Semicolon | TokenKind::RightCurlyBracket
                ) {
                    break;
                }

                if delimiter.kind == TokenKind::Function
                    && stream.slice(delimiter.range.start, delimiter.range.end)? == "global("
                {
                    let global_start = delimiter.range.start;
                    stream.next_parser_token();
                    let name_token = stream.next_parser_token().token;
                    let Some(name_range) = ident_like_range(name_token) else {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(
                                name_token.range.start,
                                name_token
                                    .range
                                    .start
                                    .saturating_add(2)
                                    .min(stream.source_end()),
                            ),
                            kind: WarningKind::Unexpected {
                                message: "Expected ident during parsing of 'composes'",
                            },
                        });
                        return Some(());
                    };
                    let right_parenthesis =
                        stream.peek_significant_skipping_comments::<true>().token;
                    if right_parenthesis.kind != TokenKind::RightParenthesis {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(
                                right_parenthesis.range.start,
                                right_parenthesis.range.end,
                            ),
                            kind: WarningKind::Unexpected {
                                message: "Expected ')' during parsing of 'composes'",
                            },
                        });
                        return Some(());
                    }
                    stream.next_parser_token();
                    item_end = right_parenthesis.range.end;
                    self.dependency_context.push_composes(
                        unescape_names(local_classes.clone()),
                        std::iter::once(unescape(stream.slice(name_range.start, name_range.end)?)),
                        Some(Cow::Borrowed("global")),
                        Range::new(global_start, item_end),
                    );
                    delimiter = stream.peek_significant_skipping_comments::<true>().token;
                    continue;
                }

                let Some(name_range) = ident_like_range(delimiter) else {
                    let name_start = delimiter.range.start;
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(
                            name_start,
                            name_start.saturating_add(2).min(stream.source_end()),
                        ),
                        kind: WarningKind::Unexpected {
                            message: "Expected ident during parsing of 'composes'",
                        },
                    });
                    return Some(());
                };
                let ident = stream.slice(name_range.start, name_range.end)?;
                if !names.is_empty() && ident.eq_ignore_ascii_case("from") {
                    stream.next_parser_token();
                    let path = stream.peek_significant_skipping_comments::<true>().token;
                    if matches!(
                        path.kind,
                        TokenKind::QuotedString | TokenKind::Ident | TokenKind::Function
                    ) {
                        let path_range = if path.kind == TokenKind::Function {
                            path.value_range
                        } else {
                            path.range
                        };
                        item_end = path_range.end;
                        self.dependency_context.push_composes(
                            unescape_names(local_classes.clone()),
                            unescape_names(std::mem::take(&mut names)),
                            Some(unescape(stream.slice(path_range.start, path_range.end)?)),
                            Range::new(item_start, item_end),
                        );
                        has_from = true;
                        stream.next_parser_token();
                        delimiter = stream.peek_significant_skipping_comments::<true>().token;
                        break;
                    }
                    names.push(ident);
                    item_end = name_range.end;
                    delimiter = path;
                    continue;
                }
                names.push(ident);
                item_end = name_range.end;
                stream.next_parser_token();
                delimiter = stream.peek_significant_skipping_comments::<true>().token;
            }

            if has_from {
                if delimiter.kind == TokenKind::Comma {
                    stream.next_parser_token();
                    replacement_end = delimiter.range.end;
                    continue;
                }
                if delimiter.kind == TokenKind::Semicolon {
                    stream.next_parser_token();
                    replacement_end = delimiter.range.end;
                    break;
                }
                if delimiter.kind == TokenKind::RightCurlyBracket {
                    replacement_end = item_end;
                    break;
                }
                replacement_end = item_end;
                break;
            }

            if delimiter.kind == TokenKind::Comma {
                if !names.is_empty() {
                    self.dependency_context.push_composes(
                        unescape_names(local_classes.clone()),
                        unescape_names(names),
                        None,
                        Range::new(item_start, item_end),
                    );
                }
                stream.next_parser_token();
                replacement_end = delimiter.range.end;
                continue;
            }

            if delimiter.kind == TokenKind::Semicolon {
                if !names.is_empty() {
                    self.dependency_context.push_composes(
                        unescape_names(local_classes.clone()),
                        unescape_names(names),
                        None,
                        Range::new(item_start, item_end),
                    );
                }
                stream.next_parser_token();
                replacement_end = delimiter.range.end;
                break;
            }

            if delimiter.kind == TokenKind::RightCurlyBracket {
                if !names.is_empty() {
                    self.dependency_context.push_composes(
                        unescape_names(local_classes.clone()),
                        unescape_names(names),
                        None,
                        Range::new(item_start, item_end),
                    );
                }
                replacement_end = item_end;
                break;
            }

            // An invalid token was encountered after a name.  The next loop
            // iteration would produce the same warning, but keeping the
            // cursor at that token preserves the legacy recovery point.
            self.handle_warning.handle_warning(Warning {
                range: Range::new(
                    delimiter.range.start,
                    delimiter
                        .range
                        .start
                        .saturating_add(2)
                        .min(stream.source_end()),
                ),
                kind: WarningKind::Unexpected {
                    message: "Expected ident during parsing of 'composes'",
                },
            });
            return Some(());
        }
        self.dependency_context
            .push_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, replacement_end),
            });
        Some(())
    }
}

impl<'s, W: HandleWarning<'s>> LexDependencies<'s, W> {
    fn handle_comment(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        if !mode_data.is_pure_mode() || end < start + 4 {
            return Some(());
        }

        let content = lexer.slice(start + 2, end - 2)?;
        if is_css_modules_magic_comment(content, "cssmodules-pure-ignore") {
            mode_data.mark_pure_ignore();
        } else if matches!(self.scope, Scope::TopLevel)
            && self.block_nesting_level == 0
            && is_css_modules_magic_comment(content, "cssmodules-pure-no-check")
        {
            mode_data.mark_pure_no_check();
        }
        Some(())
    }

    fn handle_url(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: Pos,
        end: Pos,
        content_start: Pos,
        content_end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        let value = lexer.slice(content_start, content_end)?;
        match self.scope {
            Scope::InAtImport(ref mut import_data) => {
                if import_data.in_supports() {
                    return Some(());
                }
                if import_data.url.is_some() {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(import_data.start, end),
                        kind: WarningKind::DuplicateUrl {
                            when: lexer.slice(import_data.start, end)?,
                        },
                    });
                    return Some(());
                }
                import_data.prelude.push(ImportPreludeNode::Url {
                    range: Range::new(start, end),
                });
                import_data.url = Some(value);
                import_data.url_flags = flags;
                import_data.url_range = Some(Range::new(start, end));
            }
            Scope::InBlock => self.dependency_context.push_dependency(Dependency::Url {
                request: unescape_token(value, flags),
                range: Range::new(start, end),
                kind: UrlRangeKind::Function,
            }),
            _ => {}
        }
        Some(())
    }

    fn handle_string(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
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
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(import_data.start, end),
                        kind: WarningKind::DuplicateUrl {
                            when: lexer.slice(import_data.start, end)?,
                        },
                    });
                    return Some(());
                }

                let value = lexer.slice(start + 1, end - 1)?;
                import_data.url = Some(value);
                import_data.url_flags = flags;
                // For url("inside_url") url_range will determined in right_parenthesis
                if !inside_url {
                    import_data.prelude.push(ImportPreludeNode::Url {
                        range: Range::new(start, end),
                    });
                    import_data.url_range = Some(Range::new(start, end));
                }
            }
            Scope::InBlock => {
                if let Some(mode_data) = &self.mode_data {
                    if mode_data.is_property_local_mode()
                        && matches!(
                            self.in_grid_property,
                            Some(GridPropertyKind::TemplateLike | GridPropertyKind::TemplateAreas)
                        )
                    {
                        for range in parse_grid_template_area_ranges(
                            lexer.slice(start + 1, end - 1)?,
                            start + 1,
                        ) {
                            self.dependency_context
                                .push_dependency(Dependency::LocalGridDecl {
                                    name: unescape_token(
                                        lexer.slice(range.start, range.end)?,
                                        flags,
                                    ),
                                    range,
                                });
                        }
                    }
                }
                let Some(last) = self.balanced.last() else {
                    return Some(());
                };
                let kind = match last.kind {
                    BalancedItemKind::Url => UrlRangeKind::String,
                    BalancedItemKind::ImageSet => UrlRangeKind::Function,
                    _ => return Some(()),
                };
                let value = lexer.slice(start + 1, end - 1)?;
                self.dependency_context.push_dependency(Dependency::Url {
                    request: unescape_token(value, flags),
                    range: Range::new(start, end),
                    kind,
                });
            }
            _ => {}
        }
        Some(())
    }

    fn handle_at_keyword(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        start: Pos,
        end: Pos,
        _flags: TokenFlags,
    ) -> Option<()> {
        stream.reset_at_rule_scan_state();
        let name = stream.slice_trusted(start, end);
        let kind = Self::classify_at_rule(name);
        self.scan_context = ScanContext::AtRule;
        if kind == AtRuleKind::Namespace {
            self.scope = Scope::AtNamespaceInvalid;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, end),
                kind: WarningKind::NamespaceNotSupportedInBundledCss,
            });
        } else if kind == AtRuleKind::Import {
            if !self.allow_import_at_rule {
                self.scope = Scope::AtImportInvalid;
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::NotPrecededAtImport,
                });
                return Some(());
            }
            self.scope = Scope::InAtImport(ImportData::new(start));
        } else if kind == AtRuleKind::Charset {
            self.lex_charset_at_rule(stream, start)?;
            self.scan_context = ScanContext::TopLevel;
            self.is_next_rule_prelude = true;
        } else if self.mode_data.is_some() {
            let mut can_contain_rules = true;
            if kind == AtRuleKind::Value {
                self.lex_value_at_rule(stream, start)?;
                can_contain_rules = false;
                self.scan_context = ScanContext::TopLevel;
                self.is_next_rule_prelude = true;
            } else if kind == AtRuleKind::Keyframes {
                self.lex_local_keyframes_decl(stream)?;
            } else if kind == AtRuleKind::Container {
                self.lex_local_container_at_rule(stream)?;
            } else if kind == AtRuleKind::Function {
                self.lex_local_function_decl(stream)?;
            } else if kind == AtRuleKind::Property {
                self.lex_local_dashed_ident_decl(
                    stream,
                    |name, range| Dependency::LocalPropertyDecl { name, range },
                    |range| Warning {
                        range,
                        kind: WarningKind::Unexpected {
                            message: "Expected starts with '--' during parsing of '@property'",
                        },
                    },
                    |range| Warning {
                        range,
                        kind: WarningKind::Unexpected {
                            message: "Expected '{' during parsing of '@property'",
                        },
                    },
                )?;
            } else if kind == AtRuleKind::CounterStyle {
                self.lex_local_counter_style_decl(stream)?;
            } else if kind == AtRuleKind::FontPaletteValues {
                self.lex_local_dashed_ident_decl(
                    stream,
                    |name, range| Dependency::LocalFontPaletteDecl { name, range },
                    |range| Warning {
                        range,
                        kind: WarningKind::Unexpected {
                            message: "Expected starts with '--' during parsing of '@font-palette-values'",
                        }
                    },
                    |range| Warning {
                        range,
                        kind: WarningKind::Unexpected {
                            message: "Expected '{' during parsing of '@font-palette-values'",
                        }
                    },
                )?;
            } else {
                self.is_next_rule_prelude = kind == AtRuleKind::Scope;
                if self.is_next_rule_prelude {
                    self.scan_context = ScanContext::Selector;
                }
            }

            let mode_data = self.mode_data.as_mut().unwrap();
            if can_contain_rules && self.block_nesting_level == 0 {
                mode_data.composes_local_classes.find_at_keyword();
            }

            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn handle_semicolon(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        match self.scope {
            Scope::InAtImport(ref import_data) => {
                let Some(url) = import_data.url else {
                    if let Some((name, name_range)) = import_data.prelude.icss_import_url() {
                        self.dependency_context
                            .push_dependency(Dependency::ICSSImportUrl {
                                name: unescape(name),
                                range: Range::new(import_data.start, end),
                                name_range: *name_range,
                            });
                        self.scope = Scope::TopLevel;
                        return Some(());
                    }
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(import_data.start, end),
                        kind: WarningKind::ExpectedUrl {
                            when: lexer.slice(import_data.start, end)?,
                        },
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                let Some(url_range) = &import_data.url_range else {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, end),
                        kind: WarningKind::Unexpected {
                            message: "Unexpected ';' during parsing of '@import url()'",
                        },
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                if let Some(range) = import_data.prelude.first_non_url_before(url_range) {
                    self.handle_warning.handle_warning(Warning {
                        range: *url_range,
                        kind: WarningKind::ExpectedUrlBefore {
                            when: lexer.slice(range.start, url_range.end)?,
                        },
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                }
                let layer = match &import_data.layer {
                    ImportDataLayer::None => None,
                    ImportDataLayer::EndLayer { value, range } => {
                        if url_range.start > range.start {
                            self.handle_warning.handle_warning(Warning {
                                range: *url_range,
                                kind: WarningKind::ExpectedUrlBefore {
                                    when: lexer.slice(range.start, url_range.end)?,
                                },
                            });
                            self.scope = Scope::TopLevel;
                            return Some(());
                        }
                        Some(*value)
                    }
                };
                let supports = match &import_data.supports {
                    ImportDataSupports::None => None,
                    ImportDataSupports::InSupports => {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(start, end),
                            kind: WarningKind::Unexpected {
                                message: "Unexpected ';' during parsing of 'supports()'",
                            },
                        });
                        None
                    }
                    ImportDataSupports::EndSupports { value, range } => {
                        if url_range.start > range.start {
                            self.handle_warning.handle_warning(Warning {
                                range: *url_range,
                                kind: WarningKind::ExpectedUrlBefore {
                                    when: lexer.slice(range.start, url_range.end)?,
                                },
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
                            self.handle_warning.handle_warning(Warning {
                                range: *layer_range,
                                kind: WarningKind::ExpectedLayerBefore {
                                    when: lexer.slice(supports_range.start, layer_range.end)?,
                                },
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
                self.dependency_context.push_dependency(Dependency::Import {
                    request: unescape_token(url, import_data.url_flags),
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
                if let Some(mode_data) = &mut self.mode_data {
                    mode_data.pure_global = Some(end);

                    if mode_data.is_property_local_mode() {
                        if self.in_animation_property.is_some() {
                            self.handle_local_keyframes_dependency(lexer)?;
                            self.exit_animation_property();
                        }
                        if self.in_list_style_property.is_some() {
                            self.handle_local_counter_style_dependency(lexer)?;
                            self.exit_list_style_property();
                        }
                        if self.in_font_palette_property.is_some() {
                            self.handle_local_font_palette_dependency(lexer)?;
                            self.exit_font_palette_property();
                        }
                        if self.in_container_property.is_some() {
                            self.exit_container_property();
                        }
                        if self.in_grid_property.is_some() {
                            self.exit_grid_property();
                        }
                    }
                }
                self.pending_custom_property = None;
                self.pending_grid_property = None;
                self.property_kind = PropertyKind::Generic;
                self.scan_context = ScanContext::BlockItem;
            }
            Scope::TopLevel => {
                self.is_next_rule_prelude = true;
                self.scan_context = ScanContext::TopLevel;
            }
        }
        Some(())
    }

    fn handle_function(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        let name = stream.slice_trusted(start, end);
        let item = BalancedItem::new(name, start, end);
        let at_import_top_level =
            matches!(self.scope, Scope::InAtImport(_)) && self.balanced.is_empty();
        self.balanced.push(item, self.mode_data.as_mut());

        if let Scope::InAtImport(ref mut import_data) = self.scope {
            if at_import_top_level && name.eq_ignore_ascii_case("url(") {
                import_data.prelude.push(ImportPreludeNode::Url {
                    range: Range::new(start, end),
                });
            } else if at_import_top_level && name.eq_ignore_ascii_case("layer(") {
                import_data.prelude.push(ImportPreludeNode::Layer {
                    range: Range::new(start, end),
                });
            } else if at_import_top_level && name.eq_ignore_ascii_case("supports(") {
                import_data.prelude.push(ImportPreludeNode::Supports {
                    range: Range::new(start, end),
                });
                import_data.supports = ImportDataSupports::InSupports;
            } else if at_import_top_level {
                import_data.prelude.push(ImportPreludeNode::Other {
                    range: Range::new(start, end),
                });
            } else if name.eq_ignore_ascii_case("supports(") {
                import_data.supports = ImportDataSupports::InSupports;
            }
        }

        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_current_local_mode() && name.starts_with("--") && end > start + 1 {
            self.dependency_context
                .push_dependency(Dependency::LocalFunction {
                    name: unescape_token(stream.slice(start + 2, end - 1)?, flags),
                    range: Range::new(start, end - 1),
                });
        }
        if mode_data.is_current_local_mode() && name.eq_ignore_ascii_case("var(") {
            self.lex_local_var(stream)?;
        }
        Some(())
    }

    fn handle_left_parenthesis(&mut self, _: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.balanced
            .push(BalancedItem::new_other(start, end), self.mode_data.as_mut());
        Some(())
    }

    fn handle_right_parenthesis(
        &mut self,
        lexer: &mut Lexer<'s>,
        leading_start: Pos,
        start: Pos,
        end: Pos,
    ) -> Option<()> {
        let Some(last) = self.balanced.pop(self.mode_data.as_mut()) else {
            return Some(());
        };
        if let Some(mode_data) = &mut self.mode_data {
            let mut is_function = last.kind.is_mode_function();
            let mut function_end = last.range.end;
            if last.kind.is_mode_class() {
                self.balanced.pop_mode_pseudo_class(mode_data);
                let popped = self.balanced.pop_without_moda_data().unwrap();
                debug_assert!(!matches!(
                    popped.kind,
                    BalancedItemKind::GlobalClass | BalancedItemKind::LocalClass
                ));
                is_function = popped.kind.is_mode_function();
                function_end = popped.range.end;
            }
            if is_function {
                let is_empty =
                    start == function_end || trivia_only(lexer.slice(function_end, start)?);
                let replacement_start = if is_empty {
                    function_end
                } else {
                    leading_start
                };
                if is_empty {
                    let maybe_left_parenthesis_start = function_end.saturating_sub(1);
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(maybe_left_parenthesis_start, end),
                        kind: WarningKind::Unexpected {
                            message: "':global()' or ':local()' can't be empty",
                        },
                    });
                }
                self.dependency_context
                    .push_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(replacement_start, end),
                    });
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

    fn handle_ident(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        match self.scope {
            Scope::InBlock => {
                let ident = stream.slice_trusted(start, end);
                let is_declaration_name = self.scan_context == ScanContext::DeclarationName;
                if is_declaration_name {
                    let property_local_mode = self
                        .mode_data
                        .as_ref()
                        .is_some_and(ModeData::is_property_local_mode);
                    (self.property_kind, self.pending_grid_property) =
                        Self::classify_property(ident, property_local_mode);
                    self.pending_custom_property = (self.property_kind
                        == PropertyKind::CustomProperty)
                        .then_some((Range::new(start, end), flags));
                }
                let can_reference_icss = matches!(
                    self.scan_context,
                    ScanContext::GenericValue | ScanContext::SpecialValue(_) | ScanContext::AtRule
                );
                if can_reference_icss && !self.icss_symbols.is_empty() {
                    let unescaped_ident = unescape_token(ident, flags);
                    if self.contains_icss_symbol(unescaped_ident.as_ref()) {
                        self.dependency_context
                            .push_dependency(Dependency::ICSSSymbol {
                                name: unescaped_ident,
                                range: Range::new(start, end),
                            });
                        return Some(());
                    }
                }
                let Some(mode_data) = &mut self.mode_data else {
                    return Some(());
                };
                if mode_data.is_current_local_mode()
                    && self
                        .balanced
                        .last()
                        .map(|last| last.kind.is_mode_function())
                        .unwrap_or(false)
                    && ident.starts_with("--")
                    && stream.peek_parser_token().token.kind == TokenKind::RightParenthesis
                {
                    self.dependency_context
                        .push_dependency(Dependency::LocalFunction {
                            name: unescape_token(stream.slice(start + 2, end)?, flags),
                            range: Range::new(start, end),
                        });
                    return Some(());
                }
                if mode_data.is_property_local_mode()
                    && matches!(self.scan_context, ScanContext::SpecialValue(_))
                {
                    if let Some(animation) = &mut self.in_animation_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            animation.set_rename(
                                stream.slice(start, end)?,
                                Range::new(start, end),
                                flags,
                            );
                        }
                        return Some(());
                    }

                    if let Some(list_style) = &mut self.in_list_style_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            list_style.set_rename(
                                stream.slice(start, end)?,
                                Range::new(start, end),
                                flags,
                            );
                        }
                        return Some(());
                    }

                    if let Some(font_palette) = &mut self.in_font_palette_property {
                        // Not inside functions or inside palette-mix()
                        if self.balanced.is_empty()
                            || matches!(self.balanced.last(), Some(last) if matches!(last.kind, BalancedItemKind::PaletteMix))
                        {
                            font_palette.set_rename(
                                stream.slice(start, end)?,
                                Range::new(start, end),
                                flags,
                            );
                        }
                        return Some(());
                    }

                    if let Some(container) = &mut self.in_container_property {
                        if self.balanced.is_empty() {
                            container.set_rename(ident, Range::new(start, end), flags);
                            if let Some((range, flags)) = container.take_rename(self.balanced.len())
                            {
                                self.dependency_context.push_dependency(
                                    Dependency::LocalContainerDecl {
                                        name: unescape_token(
                                            stream.slice(range.start, range.end)?,
                                            flags,
                                        ),
                                        range,
                                    },
                                );
                            }
                        }
                        return Some(());
                    }

                    if self.in_grid_property.is_some() {
                        if self.balanced.is_empty() && !is_reserved_grid_ident(ident) {
                            self.dependency_context
                                .push_dependency(Dependency::LocalGrid {
                                    name: unescape_token(ident, flags),
                                    range: Range::new(start, end),
                                });
                        }
                        return Some(());
                    }
                }

                if is_declaration_name && self.property_kind == PropertyKind::Composes {
                    if self.block_nesting_level != 1 {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(start, end),
                            kind: WarningKind::UnexpectedComposition {
                                message: "not allowed in nested rule",
                            },
                        });
                        return Some(());
                    }
                    let Some(local_classes) = mode_data
                        .composes_local_classes
                        .get_valid_local_classes(stream.lexer())
                    else {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(start, end),
                            kind: WarningKind::UnexpectedComposition {
                                message: "only allowed when selector is single :local class",
                            },
                        });
                        return Some(());
                    };
                    return self.lex_composes(stream, local_classes, start);
                }
            }
            Scope::InAtImport(ref mut import_data) => {
                if !self.balanced.is_empty() || import_data.in_supports() {
                    return Some(());
                }

                let ident = stream.slice_trusted(start, end);
                if ident.eq_ignore_ascii_case("layer") {
                    import_data.prelude.push(ImportPreludeNode::Layer {
                        range: Range::new(start, end),
                    });
                    import_data.layer = ImportDataLayer::EndLayer {
                        value: "",
                        range: Range::new(start, end),
                    }
                } else if import_data.url.is_none() && import_data.prelude.is_empty() {
                    import_data
                        .prelude
                        .push(ImportPreludeNode::IcssUrlCandidate {
                            name: ident,
                            range: Range::new(start, end),
                        });
                } else if import_data.url.is_none() {
                    import_data.prelude.push(ImportPreludeNode::Other {
                        range: Range::new(start, end),
                    });
                }
            }
            Scope::TopLevel => {
                let Some(mode_data) = &mut self.mode_data else {
                    return Some(());
                };

                mode_data.composes_local_classes.invalidate();
            }
            _ => {}
        }
        Some(())
    }

    fn handle_class(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice_trusted(start, end);
        if name == "." {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, end),
                kind: WarningKind::Unexpected {
                    message: "Invalid class selector syntax",
                },
            });
            return Some(());
        }
        if mode_data.is_current_local_mode() {
            self.dependency_context
                .push_dependency(Dependency::LocalClass {
                    name: unescape_token(name, flags),
                    range: Range::new(start, end),
                    explicit: mode_data.is_mode_explicit(),
                });
            if self.block_nesting_level == 0 {
                mode_data
                    .composes_local_classes
                    .find_local_class(start + 1, end);
            }

            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn handle_id(
        &mut self,
        lexer: &mut Lexer<'s>,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice_trusted(start, end);
        if name == "#" {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, end),
                kind: WarningKind::Unexpected {
                    message: "Invalid id selector syntax",
                },
            });
            return Some(());
        }
        if mode_data.is_current_local_mode() {
            self.dependency_context
                .push_dependency(Dependency::LocalId {
                    name: unescape_token(name, flags),
                    range: Range::new(start, end),
                    explicit: mode_data.is_mode_explicit(),
                });

            if self.block_nesting_level == 0 {
                mode_data.composes_local_classes.invalidate();
            }

            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn handle_left_curly_bracket(
        &mut self,
        _lexer: &mut Lexer<'s>,
        start: Pos,
        _: Pos,
    ) -> Option<()> {
        if matches!(self.scope, Scope::InBlock) && !self.balanced.is_empty() {
            self.balanced.push(
                BalancedItem::new_curly(start, start + 1),
                self.mode_data.as_mut(),
            );
            return Some(());
        }
        match self.scope {
            Scope::TopLevel => {
                self.allow_import_at_rule = false;
                self.scope = Scope::InBlock;
                if self.mode_data.is_none()
                    || matches!(&self.mode_data, Some(mode_data) if !matches!(mode_data.composes_local_classes.is_single, SingleLocalClass::AtKeyword))
                {
                    self.block_nesting_level = 1;
                }
            }
            Scope::InBlock => {
                let is_at_rule_block = matches!(
                    &self.mode_data,
                    Some(mode_data)
                        if matches!(mode_data.composes_local_classes.is_single, SingleLocalClass::AtKeyword)
                );
                if !is_at_rule_block {
                    self.block_nesting_level += 1;
                }
            }
            _ => return Some(()),
        }
        let (pure_check_disabled_for_selector, enter_pure_ignored_block) =
            if let Some(mode_data) = &self.mode_data {
                (
                    mode_data.is_pure_check_disabled(),
                    mode_data.pure_ignore_pending,
                )
            } else {
                (false, false)
            };
        if self.mode_data.is_none() {
            self.scan_context = ScanContext::BlockItem;
            return Some(());
        }
        if let Some(mode_data) = &mut self.mode_data {
            if mode_data.is_pure_mode()
                && !pure_check_disabled_for_selector
                && mode_data.pure_global.is_some()
            {
                let pure_global_start = mode_data.pure_global.unwrap();
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(pure_global_start, start),
                    kind: WarningKind::NotPure {
                        message: "Selector is not pure (pure selectors must contain at least one local class or id)",
                    }
                });
            }

            if enter_pure_ignored_block {
                mode_data.enter_block(self.block_nesting_level);
            }

            if mode_data.resulting_global.is_some() && mode_data.is_current_local_mode() {
                let resulting_global_start = mode_data.resulting_global.unwrap();
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(resulting_global_start, start),
                    kind: WarningKind::InconsistentModeResult,
                });
            }
            mode_data.resulting_global = None;

            self.balanced.update_property_mode(mode_data);
            self.balanced.pop_mode_pseudo_class(mode_data);
            if self.is_next_rule_prelude && self.block_nesting_level == 0 {
                let mode_data = self.mode_data.as_mut().unwrap();
                mode_data.composes_local_classes.reset_to_initial();
            }

            debug_assert!(
                self.balanced.is_empty(),
                "balanced should be empty when end of selector"
            );
        }
        self.exit_container_property();
        self.exit_grid_property();
        self.pending_custom_property = None;
        self.pending_grid_property = None;
        self.property_kind = PropertyKind::Generic;
        self.scan_context = ScanContext::BlockItem;
        Some(())
    }

    fn handle_right_curly_bracket(
        &mut self,
        lexer: &mut Lexer<'s>,
        _: Pos,
        end: Pos,
    ) -> Option<()> {
        if matches!(self.scope, Scope::InBlock) {
            if matches!(
                self.balanced.last(),
                Some(last) if matches!(last.kind, BalancedItemKind::Curly)
            ) {
                self.balanced.pop(self.mode_data.as_mut());
                return Some(());
            }

            if let Some(mode_data) = &mut self.mode_data {
                mode_data.pure_global = Some(end);

                if mode_data.is_property_local_mode() {
                    if self.in_animation_property.is_some() {
                        self.handle_local_keyframes_dependency(lexer)?;
                        self.exit_animation_property();
                    }
                    if self.in_list_style_property.is_some() {
                        self.handle_local_counter_style_dependency(lexer)?;
                        self.exit_list_style_property();
                    }
                    if self.in_font_palette_property.is_some() {
                        self.handle_local_font_palette_dependency(lexer)?;
                        self.exit_font_palette_property();
                    }
                    if self.in_container_property.is_some() {
                        self.exit_container_property();
                    }
                    if self.in_grid_property.is_some() {
                        self.exit_grid_property();
                    }
                }
            }
            if self.block_nesting_level > 0 {
                self.block_nesting_level -= 1;
            }
            if let Some(mode_data) = &mut self.mode_data {
                mode_data.clear_pure_ignore_pending();
                mode_data.exit_block(self.block_nesting_level);
            }
            if self.block_nesting_level == 0 {
                self.scope = Scope::TopLevel;
                self.scan_context = ScanContext::TopLevel;
                self.is_next_rule_prelude = true;
                if let Some(mode_data) = &mut self.mode_data {
                    mode_data.composes_local_classes.reset_to_initial();
                }
            } else {
                self.scan_context = ScanContext::BlockItem;
            }
            self.pending_custom_property = None;
            self.pending_grid_property = None;
            self.property_kind = PropertyKind::Generic;
        }
        Some(())
    }

    fn handle_pseudo_function(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        start: Pos,
        end: Pos,
        _flags: TokenFlags,
    ) -> Option<()> {
        let name = stream.slice_trusted(start, end);
        if let Some(mode_data) = &mut self.mode_data {
            if name.eq_ignore_ascii_case(":import(") {
                self.lex_icss_import(stream);
                self.dependency_context
                    .push_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start, stream.consumed_pos()),
                    });
                return Some(());
            }
            if name.eq_ignore_ascii_case(":global(") || name.eq_ignore_ascii_case(":local(") {
                if mode_data.is_inside_mode_function() {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, end),
                        kind: WarningKind::ExpectedNotInside {
                            pseudo: stream.slice(start, end)?,
                        },
                    });
                }

                let next = stream.peek_parser_token();
                if next.token.kind == TokenKind::Eof {
                    return None;
                }
                self.dependency_context
                    .push_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start, next.token.range.start),
                    });
            } else if self.block_nesting_level == 0 {
                mode_data.composes_local_classes.invalidate();
            }
        }
        self.balanced
            .push(BalancedItem::new(name, start, end), self.mode_data.as_mut());
        Some(())
    }

    fn handle_pseudo_class(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        start: Pos,
        end: Pos,
        _flags: TokenFlags,
    ) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = stream.slice_trusted(start, end);
        if name.eq_ignore_ascii_case(":global") || name.eq_ignore_ascii_case(":local") {
            if mode_data.is_inside_mode_function() {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::ExpectedNotInside {
                        pseudo: stream.slice(start, end)?,
                    },
                });
            }

            let next = stream.peek_parser_token();
            if next.token.kind == TokenKind::Eof {
                return None;
            }
            self.balanced
                .push(BalancedItem::new(name, start, end), self.mode_data.as_mut());
            self.dependency_context
                .push_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(
                        start,
                        next.leading.first_comment_start.unwrap_or(next.leading.end),
                    ),
                });
            return Some(());
        }
        if matches!(self.scope, Scope::TopLevel) && name.eq_ignore_ascii_case(":export") {
            self.lex_icss_export(stream)?;
            self.dependency_context
                .push_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, stream.consumed_pos()),
                });
            return Some(());
        }

        if self.block_nesting_level == 0 {
            mode_data.composes_local_classes.invalidate();
        }
        Some(())
    }

    fn handle_comma(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };

        if mode_data.is_pure_mode()
            && !mode_data.is_pure_check_disabled()
            && mode_data.pure_global.is_some()
        {
            let pure_global_start = mode_data.pure_global.unwrap();
            self.handle_warning.handle_warning(Warning {
                range: Range::new(pure_global_start, start),
                kind: WarningKind::NotPure {
                    message: "Selector is not pure (pure selectors must contain at least one local class or id)",
                }
            });
        }
        mode_data.pure_global = Some(end);

        if self.block_nesting_level == 0 {
            mode_data.composes_local_classes.find_comma(lexer)?;
        }

        if mode_data.resulting_global.is_some() && mode_data.is_current_local_mode() {
            let resulting_global_start = mode_data.resulting_global.unwrap();
            self.handle_warning.handle_warning(Warning {
                range: Range::new(resulting_global_start, start),
                kind: WarningKind::InconsistentModeResult,
            });
        }

        if self.balanced.len() == 1 {
            let last = self.balanced.last().unwrap();
            let is_local_class = matches!(last.kind, BalancedItemKind::LocalClass);
            let is_global_class = matches!(last.kind, BalancedItemKind::GlobalClass);
            if is_local_class || is_global_class {
                self.balanced.pop_mode_pseudo_class(mode_data);
                if mode_data.resulting_global.is_none() && is_global_class {
                    mode_data.resulting_global = Some(start);
                }
            }
        }

        if matches!(self.scope, Scope::InBlock)
            && mode_data.is_property_local_mode()
            && self.in_animation_property.is_some()
        {
            self.handle_local_keyframes_dependency(lexer)?;
        }

        Some(())
    }
}
