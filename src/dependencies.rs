use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Display;

use smallvec::smallvec;
use smallvec::SmallVec;

use crate::escape::unescape;
use crate::lexer::Token;
use crate::lexer::TokenFlags;
use crate::lexer::TokenKind;
use crate::lexer::TokenStream;
use crate::lexer::TokenWithTrivia;
use crate::HandleDependency;
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
        items: SmallVec<[ValueAtRuleImportItem<'s>; 2]>,
    },
    Value {
        local_name: &'s str,
        value: &'s str,
        has_colon: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct ValueAtRuleImportItem<'s> {
    local_name: &'s str,
    import_name: &'s str,
}

fn is_css_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn is_css_white_space_char(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn unescape_names(names: SmallVec<[&str; 2]>) -> SmallVec<[Cow<'_, str>; 2]> {
    names.into_iter().map(unescape).collect()
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

fn remove_comments_from_export_value(input: &str) -> Cow<'_, str> {
    let mut lexer = Lexer::new(input);
    let mut comments = SmallVec::<[Range; 4]>::new();
    loop {
        let token = lexer.next_token();
        if token.kind == TokenKind::Eof {
            break;
        }
        if token.kind == TokenKind::Comment {
            comments.push(token.range);
        }
    }
    if comments.is_empty() {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut copied_until = 0usize;
    for range in comments {
        let start = range.start as usize;
        let end = range.end as usize;
        output.push_str(&input[copied_until..start]);
        copied_until = end;
    }
    output.push_str(&input[copied_until..]);
    Cow::Owned(output)
}

fn parse_value_at_rule_import_item<'s>(
    input: &'s str,
    tokens: &[Token],
    start: usize,
    end: usize,
) -> ValueAtRuleImportItem<'s> {
    if start >= end {
        return ValueAtRuleImportItem {
            local_name: "",
            import_name: "",
        };
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
        return ValueAtRuleImportItem {
            local_name: token_span(input, tokens, start, index),
            import_name: token_span(input, tokens, index + 1, end),
        };
    }
    if let Some(index) = as_index {
        let import_name = token_span(input, tokens, start, index);
        let local_name = token_span(input, tokens, index + 1, end);
        if !import_name.is_empty() && !local_name.is_empty() {
            return ValueAtRuleImportItem {
                local_name,
                import_name,
            };
        }
    }

    let value = token_span(input, tokens, start, end);
    ValueAtRuleImportItem {
        local_name: value,
        import_name: value,
    }
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

fn parse_value_at_rule<'s>(input: &'s str, tokens: &[Token]) -> ValueAtRule<'s> {
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

            let mut items = SmallVec::new();
            let mut start = item_start;
            let mut item_depth = 0u32;
            for index in item_start..item_end {
                let token = tokens[index];
                if is_close_token(token.kind) {
                    item_depth = item_depth.saturating_sub(1);
                }
                if token.kind == TokenKind::Comma && item_depth == 0 {
                    let item = parse_value_at_rule_import_item(input, tokens, start, index);
                    if !item.local_name.is_empty() || !item.import_name.is_empty() {
                        items.push(item);
                    }
                    start = index + 1;
                }
                if is_open_token(token.kind) {
                    item_depth += 1;
                }
            }
            let item = parse_value_at_rule_import_item(input, tokens, start, item_end);
            if !item.local_name.is_empty() || !item.import_name.is_empty() {
                items.push(item);
            }
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

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

impl Range {
    pub fn new(start: Pos, end: Pos) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Mode {
    Local,
    Global,
    Pure,
    Css,
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

fn parse_grid_template_area_ranges(input: &str, offset: Pos) -> SmallVec<[Range; 4]> {
    let mut result = SmallVec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if start == i {
            break;
        }
        let token = &input[start..i];
        if !token.bytes().all(|c| c == b'.') {
            result.push(Range::new(offset + start as Pos, offset + i as Pos));
        }
    }

    result
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Dependency<'s> {
    Url {
        request: Cow<'s, str>,
        range: Range,
        kind: UrlRangeKind,
    },
    Import {
        request: Cow<'s, str>,
        range: Range,
        layer: Option<&'s str>,
        supports: Option<&'s str>,
        media: Option<&'s str>,
    },
    ICSSImportUrl {
        name: Cow<'s, str>,
        range: Range,
        name_range: Range,
    },
    Replace {
        content: &'s str,
        range: Range,
    },
    Charset {
        value: Cow<'s, str>,
        range: Range,
    },
    LocalClass {
        name: Cow<'s, str>,
        range: Range,
        explicit: bool,
    },
    LocalId {
        name: Cow<'s, str>,
        range: Range,
        explicit: bool,
    },
    LocalVar {
        name: Cow<'s, str>,
        range: Range,
        from: Option<Cow<'s, str>>,
    },
    LocalVarDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalPropertyDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalKeyframes {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalKeyframesDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalCounterStyle {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalCounterStyleDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalFontPalette {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalFontPaletteDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalContainer {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalContainerDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalFunction {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalFunctionDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalGrid {
        name: Cow<'s, str>,
        range: Range,
    },
    LocalGridDecl {
        name: Cow<'s, str>,
        range: Range,
    },
    Composes {
        local_classes: SmallVec<[Cow<'s, str>; 2]>,
        names: SmallVec<[Cow<'s, str>; 2]>,
        from: Option<Cow<'s, str>>,
        range: Range,
    },
    ICSSImportFrom {
        path: Cow<'s, str>,
    },
    ICSSImportValue {
        prop: Cow<'s, str>,
        value: Cow<'s, str>,
    },
    ICSSExportValue {
        prop: Cow<'s, str>,
        value: Cow<'s, str>,
    },
    ICSSSymbol {
        name: Cow<'s, str>,
        range: Range,
    },
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum UrlRangeKind {
    Function,
    String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Warning<'s> {
    range: Range,
    kind: WarningKind<'s>,
}

impl<'s> Warning<'s> {
    pub fn new(range: Range, kind: WarningKind<'s>) -> Self {
        Self { range, kind }
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn kind(&self) -> &WarningKind<'s> {
        &self.kind
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum WarningKind<'s> {
    Unexpected { message: &'s str },
    DuplicateUrl { when: &'s str },
    NamespaceNotSupportedInBundledCss,
    NotPrecededAtImport,
    ExpectedUrl { when: &'s str },
    ExpectedUrlBefore { when: &'s str },
    ExpectedLayerBefore { when: &'s str },
    InconsistentModeResult,
    ExpectedNotInside { pseudo: &'s str },
    MissingWhitespace { surrounding: &'s str },
    NotPure { message: &'s str },
    UnexpectedComposition { message: &'s str },
}

impl Display for Warning<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            WarningKind::Unexpected { message, .. } => write!(f, "{message}"),
            WarningKind::DuplicateUrl { when, .. } => write!(
                f,
                "Duplicate of 'url(...)' in '{when}'"
            ),
            WarningKind::NamespaceNotSupportedInBundledCss { .. } => write!(
                f,
                "'@namespace' is not supported in bundled CSS"
            ),
            WarningKind::NotPrecededAtImport { .. } => {
                write!(f, "Any '@import' rules must precede all other rules")
            }
            WarningKind::ExpectedUrl { when, .. } => write!(f, "Expected URL in '{when}'"),
            WarningKind::ExpectedUrlBefore { when, .. } => write!(
                f,
                "An URL in '{when}' should be before 'layer(...)' or 'supports(...)'"
            ),
            WarningKind::ExpectedLayerBefore { when, .. } => write!(
                f,
                "The 'layer(...)' in '{when}' should be before 'supports(...)'"
            ),
            WarningKind::InconsistentModeResult { .. } => write!(
                f,
                "Inconsistent rule global/local (multiple selectors must result in the same mode for the rule)"
            ),
            WarningKind::ExpectedNotInside { pseudo, .. } => write!(
                f,
                "A '{pseudo}' is not allowed inside of a ':local()' or ':global()'"
            ),
            WarningKind::MissingWhitespace { surrounding, .. } => write!(
                f,
                "Missing {surrounding} whitespace"
            ),
            WarningKind::NotPure { message, .. } => write!(f, "Pure globals is not allowed in pure mode, {message}"),
            WarningKind::UnexpectedComposition {  message, .. } => write!(f, "Composition is {message}"),
        }
    }
}

#[derive(Debug)]
pub struct LexDependencies<'s, D, W> {
    mode_data: Option<ModeData<'s>>,
    scope: Scope<'s>,
    block_nesting_level: u32,
    allow_import_at_rule: bool,
    balanced: BalancedStack,
    is_next_rule_prelude: bool,
    scan_context: ScanContext,
    property_kind: PropertyKind,
    in_animation_property: Option<InProperty<AnimationReserved>>,
    in_list_style_property: Option<InProperty<ListStyleReserved>>,
    in_font_palette_property: Option<InProperty<FontPaletteReserved>>,
    in_container_property: Option<InProperty<ContainerReserved>>,
    in_grid_property: Option<GridPropertyKind>,
    icss_symbols: HashSet<Cow<'s, str>>,
    pending_mode: Option<Mode>,
    stream_previous_kind: Option<TokenKind>,
    stream_previous_end: Pos,
    stream_previous_ends_with_separator: bool,
    stream_current_has_whitespace: bool,
    stream_pseudo_should_have_after: Option<bool>,
    handle_dependency: D,
    handle_warning: W,
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> LexDependencies<'s, D, W> {
    pub fn new(handle_dependency: D, handle_warning: W, mode: Mode) -> Self {
        Self {
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
            property_kind: PropertyKind::Generic,
            in_animation_property: None,
            in_list_style_property: None,
            in_font_palette_property: None,
            in_container_property: None,
            in_grid_property: None,
            icss_symbols: Default::default(),
            pending_mode: None,
            stream_previous_kind: None,
            stream_previous_end: 0,
            stream_previous_ends_with_separator: false,
            stream_current_has_whitespace: false,
            stream_pseudo_should_have_after: None,
            handle_dependency,
            handle_warning,
        }
    }

    /// Drive dependency extraction from the forward token stream.
    pub fn lex_streaming(&mut self, source: &mut Lexer<'s>) {
        let mut stream = crate::lexer::TokenStream::from_lexer(source);
        stream.lexer_mut().reset_scan();
        self.stream_previous_kind = None;
        self.stream_previous_end = 0;
        self.stream_previous_ends_with_separator = false;
        self.stream_current_has_whitespace = false;
        self.stream_pseudo_should_have_after = None;

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
                stream.set_lexer_position(token.range.start);
                return;
            }

            let is_trivia = matches!(token.kind, TokenKind::Comment | TokenKind::BadComment);
            let (lookahead, mode_lookahead) = if HAS_MODE && token.kind == TokenKind::Colon {
                let first = stream.peek::<KEEP_COMMENTS>();
                let mode_lookahead = if first.token.kind == TokenKind::Ident {
                    let (second, has_whitespace) = stream.peek_after_non_comment::<KEEP_COMMENTS>();
                    Some((first, second, has_whitespace))
                } else {
                    None
                };
                (Some(first.token), mode_lookahead)
            } else if HAS_MODE
                && token.kind == TokenKind::Delim
                && stream.lexer().byte_at(token.range.start) == Some(b'.')
            {
                (Some(stream.peek::<KEEP_COMMENTS>().token), None)
            } else {
                (None, None)
            };
            let buffered_comments = if KEEP_COMMENTS && token.kind == TokenKind::Colon {
                stream.take_buffered_comments()
            } else {
                Vec::new()
            };
            let mut colon_selector = None;
            let mut dot_selector = None;
            let mut clear_lookahead_at = None;
            if token.kind == TokenKind::Colon {
                if let Some(next) = lookahead {
                    let valid = matches!(next.kind, TokenKind::Ident | TokenKind::Function)
                        && next.range.start == token.range.end;
                    if valid {
                        let selector = self
                            .stream_is_selector(stream.lexer(), mode_lookahead)
                            .unwrap_or(false);
                        colon_selector = Some(selector);
                        if selector {
                            clear_lookahead_at = Some(next.range.end);
                        }
                    }
                }
            } else if token.kind == TokenKind::Delim && lookahead.is_some() {
                let selector = self
                    .stream_is_selector(stream.lexer(), None)
                    .unwrap_or(false);
                dot_selector = Some(selector);
                if selector
                    && !lookahead.is_some_and(|next| {
                        matches!(next.kind, TokenKind::Comment | TokenKind::BadComment)
                    })
                {
                    let next = lookahead.unwrap();
                    let class_end =
                        if next.kind == TokenKind::Ident && next.range.start == token.range.end {
                            next.range.end
                        } else {
                            token.range.end
                        };
                    clear_lookahead_at = Some(class_end);
                }
            }
            if let Some(position) = clear_lookahead_at {
                stream.clear_peeked_at(position);
            } else if lookahead.is_some() {
                stream.rewind_to_current();
            }
            for item in buffered_comments {
                stream.set_lexer_position(item.token.range.end);
                let _ = self.handle_comment(
                    stream.lexer_mut(),
                    item.token.range.start,
                    item.token.range.end,
                );
            }
            if !is_trivia && token.kind == TokenKind::Colon {
                let skipped_trivia_has_whitespace = stream
                    .slice(self.stream_previous_end, token.range.start)
                    .is_some_and(|input| {
                        input
                            .bytes()
                            .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c))
                    });
                self.stream_current_has_whitespace =
                    item.leading.has_whitespace() || skipped_trivia_has_whitespace;
            }

            let mut result = Some(());
            match token.kind {
                TokenKind::Comment | TokenKind::BadComment => {
                    stream.set_lexer_position(token.range.end);
                    result =
                        self.handle_comment(stream.lexer_mut(), token.range.start, token.range.end);
                }
                TokenKind::WhiteSpace => {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::QuotedString | TokenKind::BadString => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_string(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Url => {
                    stream.set_lexer_position(token.range.end);
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
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_function(
                        stream,
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Ident
                    if self.should_skip_ident(
                        stream.slice_trusted(token.range.start, token.range.end),
                    ) =>
                {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::Ident => {
                    stream.set_lexer_position(token.range.end);
                    result =
                        self.handle_ident(stream, token.range.start, token.range.end, token.flags);
                }
                TokenKind::AtKeyword => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_at_keyword(
                        stream,
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::IdHash | TokenKind::Hash if !HAS_MODE => {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::IdHash | TokenKind::Hash => {
                    let hash_end = token.range.start + 1;
                    stream.set_lexer_position(hash_end);
                    if self
                        .stream_is_selector(stream.lexer(), None)
                        .unwrap_or(false)
                    {
                        let id_end = if token.kind == TokenKind::Hash {
                            hash_end
                        } else {
                            token.range.end
                        };
                        stream.set_lexer_position(id_end);
                        result = self.handle_id(
                            stream.lexer_mut(),
                            token.range.start,
                            id_end,
                            token.flags,
                        );
                    } else {
                        stream.set_lexer_position(hash_end);
                    }
                }
                TokenKind::Delim
                    if !HAS_MODE && stream.byte_at(token.range.start) == Some(b'#') =>
                {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::Delim if stream.byte_at(token.range.start) == Some(b'#') => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_id(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                        token.flags,
                    );
                }
                TokenKind::Delim
                    if !HAS_MODE && stream.byte_at(token.range.start) == Some(b'.') =>
                {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::Delim if stream.byte_at(token.range.start) == Some(b'.') => {
                    let next = lookahead.expect("dot lookahead");
                    let class_end =
                        if next.kind == TokenKind::Ident && next.range.start == token.range.end {
                            next.range.end
                        } else {
                            token.range.end
                        };
                    stream.set_lexer_position(token.range.end);
                    if dot_selector.unwrap_or(false) {
                        stream.set_lexer_position(class_end);
                        let class_flags = if next.kind == TokenKind::Ident {
                            next.flags
                        } else {
                            token.flags
                        };
                        result = self.handle_class(
                            stream.lexer_mut(),
                            token.range.start,
                            class_end,
                            class_flags,
                        );
                    } else {
                        stream.set_lexer_position(token.range.end);
                    }
                }
                TokenKind::Colon if !HAS_MODE => {
                    stream.set_lexer_position(token.range.end);
                }
                TokenKind::Colon => {
                    let next = lookahead.expect("colon lookahead");
                    if !matches!(next.kind, TokenKind::Ident | TokenKind::Function)
                        || next.range.start != token.range.end
                    {
                        if self.scan_context == ScanContext::DeclarationName {
                            self.set_value_scan_context();
                        }
                        stream.set_lexer_position(token.range.end);
                        self.update_stream_history(stream.lexer(), token, token.range.end);
                        continue;
                    }
                    let (end, function) = (next.range.end, next.kind == TokenKind::Function);
                    stream.set_lexer_position(token.range.end);
                    if !colon_selector.unwrap_or(false) {
                        if self.scan_context == ScanContext::DeclarationName {
                            self.set_value_scan_context();
                        }
                        stream.set_lexer_position(token.range.end);
                        self.update_stream_history(stream.lexer(), token, token.range.end);
                        continue;
                    }
                    stream.set_lexer_position(end);
                    self.stream_pseudo_should_have_after = Some(
                        self.stream_previous_kind.is_none()
                            || self.stream_previous_kind.is_some_and(|previous| {
                                matches!(
                                    previous,
                                    TokenKind::Function
                                        | TokenKind::LeftParenthesis
                                        | TokenKind::Comma
                                        | TokenKind::Semicolon
                                        | TokenKind::LeftCurlyBracket
                                        | TokenKind::RightCurlyBracket
                                )
                            })
                            || self.stream_previous_ends_with_separator
                            || self.stream_current_has_whitespace,
                    );
                    result = if function {
                        self.handle_pseudo_function(stream, token.range.start, end, next.flags)
                    } else {
                        self.handle_pseudo_class(stream, token.range.start, end, next.flags)
                    };
                }
                TokenKind::LeftParenthesis => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_left_parenthesis(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::RightParenthesis => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_right_parenthesis(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::Comma => {
                    stream.set_lexer_position(token.range.end);
                    result =
                        self.handle_comma(stream.lexer_mut(), token.range.start, token.range.end);
                }
                TokenKind::Semicolon => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_semicolon(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::LeftCurlyBracket => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_left_curly_bracket(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                TokenKind::RightCurlyBracket => {
                    stream.set_lexer_position(token.range.end);
                    result = self.handle_right_curly_bracket(
                        stream.lexer_mut(),
                        token.range.start,
                        token.range.end,
                    );
                }
                _ => {
                    stream.set_lexer_position(token.range.end);
                }
            }

            let sync_position = stream.position().unwrap_or(token.range.end);
            stream.lexer_mut().sync_scan_position(sync_position);
            if result.is_none() {
                return;
            }
            if !is_trivia {
                self.update_stream_history(stream.lexer(), token, sync_position);
            }
            if !is_trivia && self.scan_context == ScanContext::GenericValue {
                self.fast_forward_generic_value(stream.lexer_mut());
            }
        }
    }

    #[inline]
    fn should_skip_ident(&self, ident: &str) -> bool {
        if matches!(self.scope, Scope::InAtImport(_))
            || matches!(
                self.scan_context,
                ScanContext::DeclarationName | ScanContext::SpecialValue(_) | ScanContext::AtRule
            )
        {
            return false;
        }
        let is_candidate_context = match self.scan_context {
            ScanContext::Selector => true,
            ScanContext::GenericValue => Self::classify_property(ident) == PropertyKind::Generic,
            ScanContext::TopLevel => self.is_next_rule_prelude,
            _ => false,
        };
        is_candidate_context
            && self.icss_symbols.is_empty()
            && self.in_animation_property.is_none()
            && self.in_list_style_property.is_none()
            && self.in_font_palette_property.is_none()
            && self.in_container_property.is_none()
            && self.in_grid_property.is_none()
    }

    #[inline]
    fn is_special_value_ident(ident: &str) -> bool {
        ident.starts_with("--") || Self::classify_property(ident) != PropertyKind::Generic
    }

    #[inline]
    fn fast_forward_generic_value(&self, lexer: &mut Lexer<'s>) {
        if self.scan_context == ScanContext::GenericValue
            && self.icss_symbols.is_empty()
            && self.in_animation_property.is_none()
            && self.in_list_style_property.is_none()
            && self.in_font_palette_property.is_none()
            && self.in_container_property.is_none()
            && self.in_grid_property.is_none()
        {
            lexer.fast_forward_generic_value(Self::is_special_value_ident);
        }
    }

    #[inline]
    fn set_value_scan_context(&mut self) {
        self.scan_context = ScanContext::for_property(self.property_kind);
    }

    fn classify_property(name: &str) -> PropertyKind {
        if name.starts_with("--") {
            return PropertyKind::CustomProperty;
        }
        if name.eq_ignore_ascii_case("composes") || name.eq_ignore_ascii_case("compose-with") {
            return PropertyKind::Composes;
        }
        if name.eq_ignore_ascii_case("animation")
            || name.eq_ignore_ascii_case("animation-name")
            || with_vendor_prefixed_eq(name, "animation", false)
            || with_vendor_prefixed_eq(name, "animation-name", false)
        {
            return PropertyKind::Animation;
        }
        if name.eq_ignore_ascii_case("list-style") || name.eq_ignore_ascii_case("list-style-type") {
            return PropertyKind::ListStyle;
        }
        if name.eq_ignore_ascii_case("font-palette") {
            return PropertyKind::FontPalette;
        }
        if name.eq_ignore_ascii_case("container") || name.eq_ignore_ascii_case("container-name") {
            return PropertyKind::Container;
        }
        if grid_property_kind(name).is_some() {
            return PropertyKind::Grid;
        }
        PropertyKind::Generic
    }

    fn update_stream_history(&mut self, lexer: &Lexer<'s>, token: Token, sync_position: Pos) {
        self.stream_previous_kind = Some(token.kind);
        self.stream_previous_end = sync_position;
        self.stream_previous_ends_with_separator = matches!(
            token.kind,
            TokenKind::Function
                | TokenKind::LeftParenthesis
                | TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::LeftCurlyBracket
                | TokenKind::RightCurlyBracket
        ) || (sync_position != token.range.end
            && sync_position
                .checked_sub(1)
                .and_then(|position| lexer.byte_at(position))
                .is_some_and(|value| matches!(value, b'(' | b',' | b';' | b'{' | b'}')));
    }

    fn stream_is_selector(
        &mut self,
        lexer: &Lexer<'s>,
        mode_lookahead: Option<(TokenWithTrivia, TokenWithTrivia, bool)>,
    ) -> Option<bool> {
        if self.is_next_rule_prelude || self.pending_mode.is_some() {
            return Some(true);
        }
        self.pending_mode = self.detect_pending_mode(lexer, mode_lookahead)?;
        Some(self.pending_mode.is_some())
    }

    fn is_next_nested_syntax(&mut self, lexer: &mut Lexer<'s>) -> Option<bool> {
        // Keep trivia in the streaming pass so magic comments are handled by
        // the normal comment callback exactly once.  The classifier only needs
        // the first byte of the next significant item; tokenizing a complete
        // property name here would make the main stream scan it twice.
        lexer.next_significant_is_not_ident(lexer.position()?)
    }

    fn detect_pending_mode(
        &self,
        lexer: &Lexer<'s>,
        mode_lookahead: Option<(TokenWithTrivia, TokenWithTrivia, bool)>,
    ) -> Option<Option<Mode>> {
        if self.mode_data.is_none() {
            return Some(None);
        }
        let Some((name_token, next_token, has_whitespace)) = mode_lookahead else {
            return Some(None);
        };
        if name_token.token.kind != TokenKind::Ident {
            return Some(None);
        }
        let name = lexer.slice_trusted(name_token.token.range.start, name_token.token.range.end);
        let mode = if name.eq_ignore_ascii_case("local") {
            Mode::Local
        } else if name.eq_ignore_ascii_case("global") {
            Mode::Global
        } else {
            return Some(None);
        };

        if has_whitespace || next_token.leading.has_whitespace() {
            return Some(None);
        }
        let next = next_token.token;
        let is_selector_start = match next.kind {
            TokenKind::IdHash | TokenKind::Hash | TokenKind::LeftCurlyBracket => true,
            TokenKind::Delim => lexer
                .byte_at(next.range.start)
                .is_some_and(|value| matches!(value, b'.' | b'#')),
            _ => false,
        };
        Some(is_selector_start.then_some(mode))
    }

    fn set_direct_pseudo_whitespace(
        &mut self,
        stream: &TokenStream<'_, 's>,
        before: Pos,
        start: Pos,
    ) -> Option<()> {
        let input = stream.slice(before, start)?;
        self.stream_pseudo_should_have_after = Some(
            input
                .bytes()
                .any(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)),
        );
        Some(())
    }

    fn pseudo_should_have_after_whitespace(&mut self, _: &Lexer<'s>, _: Pos) -> Option<bool> {
        Some(self.stream_pseudo_should_have_after.take().unwrap_or(true))
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
            stream.set_position(string.range.start);
            return Some(());
        }

        let next = stream.next_parser_token().token;
        if next.kind == TokenKind::Semicolon {
            self.handle_dependency
                .handle_dependency(Dependency::Charset {
                    value: unescape_token(
                        stream.slice(string.value_range.start, string.value_range.end)?,
                        string.flags,
                    ),
                    range: Range::new(start, next.range.end),
                });
            stream.set_position(next.range.end);
        } else {
            stream.set_position(next.range.start);
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
            stream.set_position(right_parenthesis.range.start);
            self.handle_warning.handle_warning(Warning {
                range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected ')' during parsing of ':import()'",
                },
            });
            return Some(());
        }
        self.handle_dependency
            .handle_dependency(Dependency::ICSSImportFrom {
                path: unescape(stream.slice(start, end)?),
            });
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            stream.set_position(left_curly.range.start);
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of ':import()'",
                },
            });
            return Some(());
        }
        stream.set_position(left_curly.range.end);
        loop {
            let first = stream.next_parser_token().token;
            if first.kind == TokenKind::Eof {
                return None;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                stream.set_position(first.range.end);
                break;
            }
            let prop_start = first.range.start;
            stream.set_position(prop_start);
            self.consume_icss_export_prop(stream)?;
            let prop_end = stream.position()?;
            let colon = stream.next_parser_token().token;
            if colon.kind != TokenKind::Colon {
                stream.set_position(colon.range.start);
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(colon.range.start, colon.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':' during parsing of ':import'",
                    },
                });
                return Some(());
            }
            let value_start_token = stream.next_parser_token().token;
            if value_start_token.kind == TokenKind::Eof {
                return None;
            }
            let value_start = value_start_token.range.start;
            stream.set_position(value_start);
            self.consume_icss_export_value(stream)?;
            let value_end = stream.position()?;
            let delimiter = stream.next_parser_token().token;
            if delimiter.kind == TokenKind::Semicolon {
                stream.set_position(delimiter.range.end);
            } else {
                stream.set_position(delimiter.range.start);
            }
            self.handle_dependency
                .handle_dependency(Dependency::ICSSImportValue {
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
            self.icss_symbols.insert(unescape(
                stream
                    .slice(prop_start, prop_end)?
                    .trim_end_matches(is_css_white_space_char),
            ));
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
        let right_parenthesis = if first.kind == TokenKind::RightParenthesis {
            first
        } else {
            end = first.range.end;
            loop {
                let token = stream.next_parser_token().token;
                if token.kind == TokenKind::Eof {
                    return None;
                }
                if token.kind == TokenKind::RightParenthesis {
                    break token;
                }
                if first.kind != TokenKind::QuotedString {
                    end = token.range.end;
                }
            }
        };

        stream.set_position(right_parenthesis.range.start);
        Some((start, end))
    }

    fn consume_icss_export_prop(&self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        loop {
            let token = stream.next_parser_token();
            if token.token.kind == TokenKind::Eof {
                return None;
            }
            if token.leading.first_comment_start.is_some() {
                stream.set_position(token.leading.first_comment_start.unwrap());
                return Some(());
            }
            if matches!(
                token.token.kind,
                TokenKind::Colon | TokenKind::RightCurlyBracket | TokenKind::Semicolon
            ) {
                stream.set_position(token.token.range.start);
                return Some(());
            }
        }
    }

    fn consume_icss_export_value(&self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        loop {
            let token = stream.next_parser_token().token;
            if token.kind == TokenKind::Eof {
                return None;
            }
            if matches!(
                token.kind,
                TokenKind::RightCurlyBracket | TokenKind::Semicolon
            ) {
                stream.set_position(token.range.start);
                return Some(());
            }
        }
    }

    fn consume_value_at_rule_params(
        &self,
        stream: &mut TokenStream<'_, 's>,
    ) -> Option<(Pos, Pos, SmallVec<[Token; 16]>)> {
        let start = stream.position()?;
        let mut tokens = SmallVec::new();
        let mut depth = 0u32;
        loop {
            let token = stream.next_parser_token().token;
            if token.kind == TokenKind::Eof {
                return None;
            }
            if is_close_token(token.kind) {
                depth = depth.saturating_sub(1);
            }
            if depth == 0 && token.kind == TokenKind::Semicolon {
                stream.set_position(token.range.end);
                return Some((start, token.range.start, tokens));
            }
            if depth == 0 && token.kind == TokenKind::RightCurlyBracket {
                stream.set_position(token.range.start);
                return Some((start, token.range.start, tokens));
            }
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
        let at_rule_end = params_end.max(stream.position().unwrap_or(params_end));
        let params = stream.slice(params_start, params_end)?;
        match parse_value_at_rule(params, &tokens) {
            ValueAtRule::Import { from, items } => {
                if from.is_empty() || items.is_empty() {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, at_rule_end),
                        kind: WarningKind::Unexpected {
                            message: "Broken '@value' at-rule",
                        },
                    });
                } else {
                    self.handle_dependency
                        .handle_dependency(Dependency::ICSSImportFrom {
                            path: unescape(from),
                        });
                    for item in items {
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSImportValue {
                                prop: unescape(item.local_name),
                                value: unescape(item.import_name),
                            });
                        self.icss_symbols.insert(unescape(item.local_name));
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSExportValue {
                                prop: unescape(item.local_name),
                                value: unescape(item.local_name),
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
                    self.handle_dependency
                        .handle_dependency(Dependency::ICSSExportValue {
                            prop: unescape(local_name),
                            value: Cow::Borrowed(value),
                        });
                    self.icss_symbols.insert(unescape(local_name));
                }
            }
        }
        self.handle_dependency
            .handle_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, at_rule_end),
            });
        Some(())
    }

    fn lex_icss_export(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            stream.set_position(left_curly.range.start);
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of ':export'",
                },
            });
            return Some(());
        }
        stream.set_position(left_curly.range.end);
        loop {
            let first = stream.next_parser_token().token;
            if first.kind == TokenKind::Eof {
                return None;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                stream.set_position(first.range.end);
                break;
            }
            let prop_start = first.range.start;
            stream.set_position(prop_start);
            self.consume_icss_export_prop(stream)?;
            let prop_end = stream.position()?;
            let colon = stream.next_parser_token().token;
            if colon.kind != TokenKind::Colon {
                stream.set_position(colon.range.start);
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(colon.range.start, colon.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':' during parsing of ':export'",
                    },
                });
                return Some(());
            }
            let value_start_token = stream.next_parser_token().token;
            if value_start_token.kind == TokenKind::Eof {
                return None;
            }
            let value_start = value_start_token.range.start;
            stream.set_position(value_start);
            self.consume_icss_export_value(stream)?;
            let value_end = stream.position()?;
            let delimiter = stream.next_parser_token().token;
            if delimiter.kind == TokenKind::Semicolon {
                stream.set_position(delimiter.range.end);
            } else {
                stream.set_position(delimiter.range.start);
            }
            self.handle_dependency
                .handle_dependency(Dependency::ICSSExportValue {
                    prop: unescape(
                        stream
                            .slice(prop_start, prop_end)?
                            .trim_end_matches(is_css_white_space_char),
                    ),
                    value: remove_comments_from_export_value(
                        stream
                            .slice(value_start, value_end)?
                            .trim_end_matches(is_css_white_space_char),
                    ),
                });
            self.icss_symbols.insert(unescape(
                stream
                    .slice(prop_start, prop_end)?
                    .trim_end_matches(is_css_white_space_char),
            ));
        }
        Some(())
    }

    fn lex_local_var(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let cursor_start = stream.position()?;
        let name_token = stream.next_parser_token().token;
        let name = stream.slice(name_token.range.start, name_token.range.end)?;
        let start = name_token.range.start;
        if name_token.kind != TokenKind::Ident || !name.starts_with("--") {
            stream.set_position(cursor_start);
            self.handle_warning.handle_warning(Warning {
                kind: WarningKind::Unexpected {
                    message: "Expected starts with '--' during parsing of 'var()'",
                },
                range: Range::new(start, (start + 2).min(name_token.range.end)),
            });
            return Some(());
        }
        let end = name_token.range.end;
        let next = stream.next_parser_token();
        let from = if next.token.kind == TokenKind::Ident
            && stream.slice(next.token.range.start, next.token.range.end)? == "from"
        {
            let path = stream.next_parser_token();
            let path_start = path.token.range.start;
            let path_end = path.token.range.end;
            if !matches!(path.token.kind, TokenKind::QuotedString | TokenKind::Ident) {
                stream.set_position(cursor_start);
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(path_start, path_end),
                    kind: WarningKind::Unexpected {
                        message: "Expected string or ident during parsing of 'composes'",
                    },
                });
                return Some(());
            }
            stream.set_position(path_end);
            Some(unescape_token(
                stream.slice(path_start, path_end)?,
                path.token.flags,
            ))
        } else {
            stream.set_position(next.token.range.start);
            None
        };
        self.handle_dependency
            .handle_dependency(Dependency::LocalVar {
                name: unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                range: Range::new(start, end),
                from,
            });
        Some(())
    }

    fn lex_local_var_decl(
        &mut self,
        stream: &mut TokenStream<'_, 's>,
        name: &'s str,
        start: Pos,
        end: Pos,
        flags: TokenFlags,
    ) -> Option<()> {
        let colon = stream.next_parser_token().token;
        if colon.kind != TokenKind::Colon {
            stream.set_position(colon.range.start);
            return Some(());
        }
        stream.set_position(colon.range.end);
        self.handle_dependency
            .handle_dependency(Dependency::LocalVarDecl {
                name: unescape_token(name, flags),
                range: Range::new(start, end),
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
        let name = stream.slice(start, name_token.range.end)?;
        if name_token.kind != TokenKind::Ident || !name.starts_with("--") {
            self.handle_warning
                .handle_warning(dashed_warning(Range::new(
                    start,
                    (start + 2).min(name_token.range.end),
                )));
            return Some(());
        }
        let end = name_token.range.end;
        self.handle_dependency
            .handle_dependency(local_decl_dependency(
                unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                Range::new(start, end),
            ));
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning
                .handle_warning(left_curly_warning(Range::new(
                    left_curly.range.start,
                    left_curly.range.end,
                )));
            return Some(());
        }
        stream.set_position(left_curly.range.start);
        Some(())
    }

    fn lex_local_keyframes_decl(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let mut is_function = false;
        let prelude_start = stream.position()?;
        let first = stream.next_parser_token().token;
        let name_token = if first.kind == TokenKind::Colon {
            let pseudo_start = first.range.start;
            self.set_direct_pseudo_whitespace(stream, prelude_start, pseudo_start)?;
            let pseudo_name = stream.next_parser_token().token;
            let pseudo_end = if matches!(pseudo_name.kind, TokenKind::Ident | TokenKind::Function) {
                pseudo_name.range.end
            } else {
                first.range.end
            };
            stream.set_lexer_position(pseudo_end);
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
        stream.set_lexer_position(end);
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            let name = stream.slice(start, end)?;
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframesDecl {
                    name: unescape_token(name, name_token.flags),
                    range: Range::new(start, end),
                });
        }
        if is_function {
            stream.set_lexer_position(end);
            let right_parenthesis = stream.next_parser_token().token;
            if right_parenthesis.kind != TokenKind::RightParenthesis {
                stream.set_position(right_parenthesis.range.start);
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ')' during parsing of '@keyframes :local(' or '@keyframes :global('",
                    },
                });
                return Some(());
            }
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                });
            let mode_data = self.mode_data.as_mut().unwrap();
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
            stream.set_lexer_position(right_parenthesis.range.end);
        }
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            stream.set_position(left_curly.range.start);
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@keyframes'",
                },
            });
            return Some(());
        }
        stream.set_position(left_curly.range.start);
        Some(())
    }

    fn handle_local_keyframes_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let animation = self.in_animation_property.as_mut().unwrap();
        if let Some((range, flags)) = animation.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframes {
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
        self.handle_dependency
            .handle_dependency(Dependency::LocalCounterStyleDecl {
                name: unescape_token(stream.slice(start, end)?, name_token.flags),
                range: Range::new(start, end),
            });
        let left_curly = stream.next_parser_token().token;
        if left_curly.kind != TokenKind::LeftCurlyBracket {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_curly.range.start, left_curly.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@counter-style'",
                },
            });
            return Some(());
        }
        stream.set_position(left_curly.range.start);
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
        stream.set_lexer_position(end);
        if is_reserved_container_query_ident(ident) {
            return Some(());
        }
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalContainer {
                    name: unescape_token(ident, name_token.flags),
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn lex_local_function_decl(&mut self, stream: &mut TokenStream<'_, 's>) -> Option<()> {
        let mut is_function = false;
        let prelude_start = stream.position()?;
        let first = stream.next_parser_token().token;
        let name_token = if first.kind == TokenKind::Colon {
            let pseudo_start = first.range.start;
            self.set_direct_pseudo_whitespace(stream, prelude_start, pseudo_start)?;
            let pseudo_name = stream.next_parser_token().token;
            let pseudo_end = if matches!(pseudo_name.kind, TokenKind::Ident | TokenKind::Function) {
                pseudo_name.range.end
            } else {
                first.range.end
            };
            stream.set_lexer_position(pseudo_end);
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
            is_function =
                pseudo.eq_ignore_ascii_case(":local(") || pseudo.eq_ignore_ascii_case(":global(");
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
        stream.set_lexer_position(end);
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalFunctionDecl {
                    name: unescape_token(stream.slice(start + 2, end)?, name_token.flags),
                    range: Range::new(start, end),
                });
        }

        if is_function {
            stream.set_lexer_position(end);
            let right_parenthesis = stream.next_parser_token().token;
            if right_parenthesis.kind != TokenKind::RightParenthesis {
                stream.set_position(right_parenthesis.range.start);
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
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(right_parenthesis.range.start, right_parenthesis.range.end),
                });
            let mode_data = self.mode_data.as_mut().unwrap();
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
            stream.set_lexer_position(right_parenthesis.range.end);
        }

        let left_parenthesis = stream.next_parser_token().token;
        if left_parenthesis.kind != TokenKind::LeftParenthesis {
            stream.set_position(left_parenthesis.range.start);
            self.handle_warning.handle_warning(Warning {
                range: Range::new(left_parenthesis.range.start, left_parenthesis.range.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '(' during parsing of '@function'",
                },
            });
        } else {
            stream.set_position(left_parenthesis.range.start);
        }
        Some(())
    }

    fn handle_local_counter_style_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let list_style = self.in_list_style_property.as_mut().unwrap();
        if let Some((range, flags)) = list_style.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalCounterStyle {
                    name: unescape_token(lexer.slice(range.start, range.end)?, flags),
                    range,
                });
        }
        Some(())
    }

    fn handle_local_font_palette_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        let font_palette = self.in_font_palette_property.as_mut().unwrap();
        if let Some((range, flags)) = font_palette.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalFontPalette {
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
        let colon = stream.next_parser_token().token;
        if colon.kind != TokenKind::Colon {
            stream.set_position(colon.range.start);
            return Some(());
        }

        stream.set_position(colon.range.end);
        let mut replacement_end = colon.range.end;
        loop {
            let first = stream.next_parser_token().token;
            if first.kind == TokenKind::Eof {
                stream.set_position(first.range.start);
                break;
            }
            if first.kind == TokenKind::RightCurlyBracket {
                stream.set_position(first.range.start);
                break;
            }
            if first.kind == TokenKind::Semicolon {
                replacement_end = first.range.end;
                stream.set_position(first.range.end);
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
                    let right_parenthesis = stream.next_parser_token().token;
                    if right_parenthesis.kind != TokenKind::RightParenthesis {
                        stream.set_position(right_parenthesis.range.start);
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
                    item_end = right_parenthesis.range.end;
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: unescape_names(local_classes.clone()),
                            names: smallvec![unescape(
                                stream.slice(name_range.start, name_range.end,)?
                            )],
                            from: Some(Cow::Borrowed("global")),
                            range: Range::new(global_start, item_end),
                        });
                } else {
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
                    if delimiter.kind == TokenKind::Function {
                        stream.set_position(name_range.end);
                    }
                    if !names.is_empty() && ident.eq_ignore_ascii_case("from") {
                        let path = stream.next_parser_token().token;
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
                            self.handle_dependency
                                .handle_dependency(Dependency::Composes {
                                    local_classes: unescape_names(local_classes.clone()),
                                    names: unescape_names(std::mem::take(&mut names)),
                                    from: Some(unescape(
                                        stream.slice(path_range.start, path_range.end)?,
                                    )),
                                    range: Range::new(item_start, item_end),
                                });
                            has_from = true;
                            let next = stream.next_parser_token().token;
                            delimiter = next;
                            break;
                        }
                        names.push(ident);
                        item_end = name_range.end;
                        delimiter = path;
                        continue;
                    }
                    names.push(ident);
                    item_end = name_range.end;
                }
                delimiter = stream.next_parser_token().token;
            }

            if has_from {
                if delimiter.kind == TokenKind::Comma {
                    stream.set_position(delimiter.range.end);
                    replacement_end = delimiter.range.end;
                    continue;
                }
                if delimiter.kind == TokenKind::Semicolon {
                    replacement_end = delimiter.range.end;
                    stream.set_position(delimiter.range.end);
                    break;
                }
                if delimiter.kind == TokenKind::RightCurlyBracket {
                    replacement_end = item_end;
                    stream.set_position(delimiter.range.start);
                    break;
                }
                replacement_end = item_end;
                stream.set_position(delimiter.range.start);
                break;
            }

            if delimiter.kind == TokenKind::Comma {
                if !names.is_empty() {
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: unescape_names(local_classes.clone()),
                            names: unescape_names(names),
                            from: None,
                            range: Range::new(item_start, item_end),
                        });
                }
                stream.set_position(delimiter.range.end);
                replacement_end = delimiter.range.end;
                continue;
            }

            if delimiter.kind == TokenKind::Semicolon {
                if !names.is_empty() {
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: unescape_names(local_classes.clone()),
                            names: unescape_names(names),
                            from: None,
                            range: Range::new(item_start, item_end),
                        });
                }
                replacement_end = delimiter.range.end;
                stream.set_position(delimiter.range.end);
                break;
            }

            if delimiter.kind == TokenKind::RightCurlyBracket {
                if !names.is_empty() {
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: unescape_names(local_classes.clone()),
                            names: unescape_names(names),
                            from: None,
                            range: Range::new(item_start, item_end),
                        });
                }
                replacement_end = item_end;
                stream.set_position(delimiter.range.start);
                break;
            }

            // An invalid token was encountered after a name.  The next loop
            // iteration would produce the same warning, but keeping the
            // cursor at that token preserves the legacy recovery point.
            stream.set_position(delimiter.range.start);
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
        self.handle_dependency
            .handle_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, replacement_end),
            });
        Some(())
    }
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> LexDependencies<'s, D, W> {
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
            Scope::InBlock => self.handle_dependency.handle_dependency(Dependency::Url {
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
                            self.handle_dependency
                                .handle_dependency(Dependency::LocalGridDecl {
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
                self.handle_dependency.handle_dependency(Dependency::Url {
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
        let name = stream.slice_trusted(start, end);
        self.scan_context = ScanContext::AtRule;
        if name.eq_ignore_ascii_case("@namespace") {
            self.scope = Scope::AtNamespaceInvalid;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, end),
                kind: WarningKind::NamespaceNotSupportedInBundledCss,
            });
        } else if name.eq_ignore_ascii_case("@import") {
            if !self.allow_import_at_rule {
                self.scope = Scope::AtImportInvalid;
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::NotPrecededAtImport,
                });
                return Some(());
            }
            self.scope = Scope::InAtImport(ImportData::new(start));
        } else if name.eq_ignore_ascii_case("@charset") {
            self.lex_charset_at_rule(stream, start)?;
        } else if self.mode_data.is_some() {
            let mut can_contain_rules = true;
            if name.eq_ignore_ascii_case("@value") {
                self.lex_value_at_rule(stream, start)?;
                can_contain_rules = false;
            } else if name.eq_ignore_ascii_case("@keyframes")
                || with_vendor_prefixed_eq(name, "keyframes", true)
            {
                self.lex_local_keyframes_decl(stream)?;
            } else if name.eq_ignore_ascii_case("@container") {
                self.lex_local_container_at_rule(stream)?;
            } else if name.eq_ignore_ascii_case("@function") {
                self.lex_local_function_decl(stream)?;
            } else if name.eq_ignore_ascii_case("@property") {
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
            } else if name.eq_ignore_ascii_case("@counter-style") {
                self.lex_local_counter_style_decl(stream)?;
            } else if name.eq_ignore_ascii_case("@font-palette-values") {
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
                self.is_next_rule_prelude = name.eq_ignore_ascii_case("@scope");
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
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSImportUrl {
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
                self.handle_dependency
                    .handle_dependency(Dependency::Import {
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
                if self.mode_data.is_none() {
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
                self.pending_mode = None;
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
                self.scan_context = if self.is_next_rule_prelude {
                    ScanContext::Selector
                } else {
                    ScanContext::DeclarationName
                };
            }
            Scope::TopLevel => {
                if self.mode_data.is_none() {
                    return Some(());
                }
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
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
            self.handle_dependency
                .handle_dependency(Dependency::LocalFunction {
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
        start: Pos,
        end: Pos,
    ) -> Option<()> {
        let Some(last) = self.balanced.pop(self.mode_data.as_mut()) else {
            return Some(());
        };
        if let Some(mode_data) = &mut self.mode_data {
            let mut is_function = last.kind.is_mode_function();
            if last.kind.is_mode_class() {
                self.balanced.pop_mode_pseudo_class(mode_data);
                let popped = self.balanced.pop_without_moda_data().unwrap();
                debug_assert!(!matches!(
                    popped.kind,
                    BalancedItemKind::GlobalClass | BalancedItemKind::LocalClass
                ));
                is_function = popped.kind.is_mode_function();
            }
            if is_function {
                let replacement_start = self.stream_previous_end.min(start);
                let maybe_left_parenthesis_start = replacement_start.saturating_sub(1);
                if replacement_start > 0
                    && lexer.slice(maybe_left_parenthesis_start, replacement_start)? == "("
                {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(maybe_left_parenthesis_start, end),
                        kind: WarningKind::Unexpected {
                            message: "':global()' or ':local()' can't be empty",
                        },
                    });
                }
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
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
                let Some(mode_data) = &mut self.mode_data else {
                    return Some(());
                };

                let ident = stream.slice_trusted(start, end);
                if self.scan_context == ScanContext::DeclarationName {
                    self.property_kind = Self::classify_property(ident);
                }
                if !self.icss_symbols.is_empty() {
                    let unescaped_ident = unescape_token(ident, flags);
                    if self.icss_symbols.contains(unescaped_ident.as_ref()) {
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSSymbol {
                                name: unescaped_ident,
                                range: Range::new(start, end),
                            });
                        return Some(());
                    }
                }
                if mode_data.is_current_local_mode()
                    && self
                        .balanced
                        .last()
                        .map(|last| last.kind.is_mode_function())
                        .unwrap_or(false)
                    && ident.starts_with("--")
                    && {
                        stream.set_position(end);
                        let is_right_parenthesis =
                            stream.next_parser_token().token.kind == TokenKind::RightParenthesis;
                        stream.set_position(end);
                        is_right_parenthesis
                    }
                {
                    self.handle_dependency
                        .handle_dependency(Dependency::LocalFunction {
                            name: unescape_token(stream.slice(start + 2, end)?, flags),
                            range: Range::new(start, end),
                        });
                    return Some(());
                }
                if mode_data.is_property_local_mode() {
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
                                self.handle_dependency.handle_dependency(
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
                            self.handle_dependency
                                .handle_dependency(Dependency::LocalGrid {
                                    name: unescape_token(ident, flags),
                                    range: Range::new(start, end),
                                });
                        }
                        return Some(());
                    }

                    if let Some(name) = ident.strip_prefix("--") {
                        return self.lex_local_var_decl(stream, name, start, end, flags);
                    }

                    if ident.eq_ignore_ascii_case("animation")
                        || ident.eq_ignore_ascii_case("animation-name")
                        || with_vendor_prefixed_eq(ident, "animation", false)
                        || with_vendor_prefixed_eq(ident, "animation-name", false)
                    {
                        self.enter_animation_property();
                        return Some(());
                    }

                    if ident.eq_ignore_ascii_case("list-style")
                        || ident.eq_ignore_ascii_case("list-style-type")
                    {
                        self.enter_list_style_property();
                        return Some(());
                    }

                    if ident.eq_ignore_ascii_case("font-palette") {
                        self.enter_font_palette_property();
                        return Some(());
                    }

                    if ident.eq_ignore_ascii_case("container")
                        || ident.eq_ignore_ascii_case("container-name")
                    {
                        self.enter_container_property();
                        return Some(());
                    }

                    if let Some(kind) = grid_property_kind(ident) {
                        self.enter_grid_property(kind);
                        return Some(());
                    }
                }

                if ident.eq_ignore_ascii_case("composes")
                    || ident.eq_ignore_ascii_case("compose-with")
                {
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
        if let Some(mode) = self.pending_mode.take() {
            if name != "." && mode == Mode::Local {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalClass {
                        name: unescape_token(name, flags),
                        range: Range::new(start, end),
                        explicit: true,
                    });
            }
            return Some(());
        }
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
            self.handle_dependency
                .handle_dependency(Dependency::LocalClass {
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
        if let Some(mode) = self.pending_mode.take() {
            if name != "#" && mode == Mode::Local {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalId {
                        name: unescape_token(name, flags),
                        range: Range::new(start, end),
                        explicit: true,
                    });
            }
            return Some(());
        }
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
            self.handle_dependency
                .handle_dependency(Dependency::LocalId {
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
        lexer: &mut Lexer<'s>,
        start: Pos,
        _: Pos,
    ) -> Option<()> {
        self.pending_mode = None;
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
            return Some(());
        }
        self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
        self.scan_context = if self.is_next_rule_prelude {
            ScanContext::Selector
        } else {
            ScanContext::DeclarationName
        };
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
        Some(())
    }

    fn handle_right_curly_bracket(
        &mut self,
        lexer: &mut Lexer<'s>,
        _: Pos,
        end: Pos,
    ) -> Option<()> {
        self.pending_mode = None;
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
            } else if self.mode_data.is_some() {
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
                self.scan_context = if self.is_next_rule_prelude {
                    ScanContext::Selector
                } else {
                    ScanContext::DeclarationName
                };
            }
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
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start, stream.position()?),
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

                let position = stream.position()?;
                stream.set_position(position);
                let next = stream.next_parser_token();
                if next.token.kind == TokenKind::Eof {
                    stream.set_position(next.token.range.start);
                    return None;
                }
                stream.set_position(next.token.range.start);
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
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
        let should_have_after_white_space = self
            .pseudo_should_have_after_whitespace(stream.lexer(), start)
            .unwrap_or(true);
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = stream.slice_trusted(start, end);
        if name.eq_ignore_ascii_case(":global") || name.eq_ignore_ascii_case(":local") {
            let is_non_selector_mode = self.pending_mode.is_some();
            if !is_non_selector_mode && mode_data.is_inside_mode_function() {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::ExpectedNotInside {
                        pseudo: stream.slice(start, end)?,
                    },
                });
            }

            stream.set_position(end);
            let next = stream.next_parser_token();
            if next.token.kind == TokenKind::Eof {
                stream.set_position(next.token.range.start);
                return None;
            }
            let has_after_white_space = next.leading.has_whitespace();
            let has_trailing_trivia = next.leading.end != end;
            stream.set_position(next.token.range.start);
            if next.token.kind != TokenKind::RightParenthesis
                && next.token.kind != TokenKind::Comma
                && (next.token.kind != TokenKind::LeftCurlyBracket
                    || (has_trailing_trivia && !has_after_white_space))
            {
                if should_have_after_white_space && !has_after_white_space {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, end),
                        kind: WarningKind::MissingWhitespace {
                            surrounding: "trailing",
                        },
                    });
                }
                if !should_have_after_white_space && has_after_white_space {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, end),
                        kind: WarningKind::MissingWhitespace {
                            surrounding: "leading",
                        },
                    });
                }
            }

            if !is_non_selector_mode {
                self.balanced
                    .push(BalancedItem::new(name, start, end), self.mode_data.as_mut());
            }
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
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
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, stream.position()?),
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
