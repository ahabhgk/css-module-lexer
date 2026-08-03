use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Display;

use smallvec::smallvec;
use smallvec::SmallVec;

use crate::escape;
use crate::parser::grid_template_area_ranges;
use crate::parser::parse_value_at_rule_tokens;
use crate::parser::CssParser;
use crate::parser::TokenVisitor;
use crate::parser::ValueAtRule;
use crate::tokenizer::Token;
use crate::HandleDependency;
use crate::HandleWarning;
use crate::Pos;

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
            if $(escape::eq_ignore_ascii_case(value, $pattern))||+ {
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

#[derive(Debug)]
struct ImportData<'s> {
    start: Pos,
    prelude: ImportPrelude<'s>,
    url: Option<&'s str>,
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
        if function_name_eq(name, "url") {
            Self::Url
        } else if function_name_eq(name, "image-set")
            || function_name(name)
                .is_some_and(|name| with_vendor_prefixed_eq(name, "image-set", false))
        {
            Self::ImageSet
        } else if function_name_eq(name, "layer") {
            Self::Layer
        } else if function_name_eq(name, "supports") {
            Self::Supports
        } else if function_name_eq(name, "palette-mix") {
            Self::PaletteMix
        } else if pseudo_name_eq(name, "local", true) {
            Self::LocalFn
        } else if pseudo_name_eq(name, "global", true) {
            Self::GlobalFn
        } else if pseudo_name_eq(name, "local", false) {
            Self::LocalClass
        } else if pseudo_name_eq(name, "global", false) {
            Self::GlobalClass
        } else {
            Self::Other
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

fn is_css_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
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

fn function_name(input: &str) -> Option<&str> {
    input.strip_suffix('(')
}

fn function_name_eq(input: &str, expected: &str) -> bool {
    function_name(input).is_some_and(|name| escape::eq_ignore_ascii_case(name, expected))
}

fn pseudo_name(input: &str, is_function: bool) -> Option<&str> {
    let input = input.strip_prefix(':')?;
    if is_function {
        input.strip_suffix('(')
    } else {
        Some(input)
    }
}

fn pseudo_name_eq(input: &str, expected: &str, is_function: bool) -> bool {
    pseudo_name(input, is_function).is_some_and(|name| escape::eq_ignore_ascii_case(name, expected))
}

fn at_keyword_name(input: &str) -> Option<&str> {
    input.strip_prefix('@')
}

fn at_keyword_name_eq(input: &str, expected: &str) -> bool {
    at_keyword_name(input).is_some_and(|name| escape::eq_ignore_ascii_case(name, expected))
}

fn with_vendor_prefixed_eq(left: &str, right: &str, at_rule: bool) -> bool {
    let left = escape::unescape(left);
    let left = left.as_ref();
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

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
            Mode::Global => false,
            Mode::Css => unreachable!(),
        }
    }

    pub fn is_property_local_mode(&self) -> bool {
        match self.property {
            Mode::Local | Mode::Pure => true,
            Mode::Global => false,
            Mode::Css => unreachable!(),
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
    pub fn get_valid_local_classes(
        &mut self,
        lexer: &CssParser<'s>,
    ) -> Option<SmallVec<[&'s str; 2]>> {
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

    pub fn find_comma(&mut self, lexer: &CssParser<'s>) -> Option<()> {
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
    rename: Option<Range>,
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

    pub fn set_rename(&mut self, ident: &str, range: Range) {
        if self.check_reserved(ident) {
            self.rename = Some(range);
        }
    }

    pub fn take_rename(&mut self, balanced_len: usize) -> Option<Range> {
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
        escape::unescape(ident).starts_with("--")
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
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
    ICSSImportUrl {
        name: &'s str,
        range: Range,
        name_range: Range,
    },
    Replace {
        content: &'s str,
        range: Range,
    },
    Charset {
        value: &'s str,
        range: Range,
    },
    LocalClass {
        name: &'s str,
        range: Range,
        explicit: bool,
    },
    LocalId {
        name: &'s str,
        range: Range,
        explicit: bool,
    },
    LocalVar {
        name: &'s str,
        range: Range,
        from: Option<&'s str>,
    },
    LocalVarDecl {
        name: &'s str,
        range: Range,
    },
    LocalPropertyDecl {
        name: &'s str,
        range: Range,
    },
    LocalKeyframes {
        name: &'s str,
        range: Range,
    },
    LocalKeyframesDecl {
        name: &'s str,
        range: Range,
    },
    LocalCounterStyle {
        name: &'s str,
        range: Range,
    },
    LocalCounterStyleDecl {
        name: &'s str,
        range: Range,
    },
    LocalFontPalette {
        name: &'s str,
        range: Range,
    },
    LocalFontPaletteDecl {
        name: &'s str,
        range: Range,
    },
    LocalContainer {
        name: &'s str,
        range: Range,
    },
    LocalContainerDecl {
        name: &'s str,
        range: Range,
    },
    LocalFunction {
        name: &'s str,
        range: Range,
    },
    LocalFunctionDecl {
        name: &'s str,
        range: Range,
    },
    LocalGrid {
        name: &'s str,
        range: Range,
    },
    LocalGridDecl {
        name: &'s str,
        range: Range,
    },
    Composes {
        local_classes: SmallVec<[&'s str; 2]>,
        names: SmallVec<[&'s str; 2]>,
        from: Option<&'s str>,
        range: Range,
    },
    ICSSImportFrom {
        path: &'s str,
    },
    ICSSImportValue {
        prop: &'s str,
        value: &'s str,
    },
    ICSSExportValue {
        prop: &'s str,
        value: Cow<'s, str>,
    },
    ICSSSymbol {
        name: &'s str,
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
    in_animation_property: Option<InProperty<AnimationReserved>>,
    in_list_style_property: Option<InProperty<ListStyleReserved>>,
    in_font_palette_property: Option<InProperty<FontPaletteReserved>>,
    in_container_property: Option<InProperty<ContainerReserved>>,
    in_grid_property: Option<GridPropertyKind>,
    icss_symbols: HashSet<String>,
    non_selector_mode: Option<Mode>,
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
            in_animation_property: None,
            in_list_style_property: None,
            in_font_palette_property: None,
            in_container_property: None,
            in_grid_property: None,
            icss_symbols: Default::default(),
            non_selector_mode: None,
            handle_dependency,
            handle_warning,
        }
    }

    fn is_next_nested_syntax(&mut self, lexer: &mut CssParser<'s>) -> Option<bool> {
        lexer.consume_trivia_with_visitor(self)?;
        if lexer.current_is(Token::CloseCurlyBracket) {
            return Some(false);
        }
        // If what follows is a property, then it's not a nested selector
        // This is not strictly correct, but it's good enough for our purposes
        // since we only need 'is_selector()' when next char is '#', '.', or ':'
        Some(!lexer.current_is(Token::Ident))
    }

    fn get_non_selector_mode_pseudo(&self, lexer: &CssParser<'s>) -> Option<Option<Mode>> {
        if self.mode_data.is_none() {
            return Some(None);
        }
        let mut lookahead = lexer.clone();
        let name = lookahead.advance()?;
        let mode = if lexer.token_is_keyword(name, "local") {
            Mode::Local
        } else if lexer.token_is_keyword(name, "global") {
            Mode::Global
        } else {
            return Some(None);
        };

        let trivia = lookahead.consume_trivia();
        Some(
            (!trivia.has_white_space
                && (matches!(
                    lookahead.current().map(|token| token.token),
                    Some(Token::IDHash | Token::Hash | Token::CurlyBracketBlock)
                ) || matches!(lookahead.current_raw(), Some("."))))
            .then_some(mode),
        )
    }

    fn get_media(&self, lexer: &CssParser<'s>, start: Pos, end: Pos) -> Option<&'s str> {
        lexer
            .has_non_trivia_between(start, end)
            .then(|| lexer.slice(start, end))?
    }

    fn lex_charset_at_rule(&mut self, lexer: &mut CssParser<'s>, start: Pos) -> Option<()> {
        lexer.skip_trivia();
        let token = lexer.current()?;
        if token.token != Token::QuotedString {
            return Some(());
        }
        lexer.advance();
        lexer.skip_trivia();
        if lexer.current_is(Token::Semicolon) {
            lexer.advance();
            self.handle_dependency
                .handle_dependency(Dependency::Charset {
                    value: lexer.slice(token.span.start + 1, token.span.end - 1)?,
                    range: Range::new(start, lexer.position()),
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

    fn back_white_space_and_comments_distance(
        &self,
        lexer: &CssParser<'s>,
        end: Pos,
    ) -> Option<Pos> {
        let mut position = end;
        // The closing parenthesis has already been consumed.
        for token in lexer.consumed_tokens().iter().rev().skip(1) {
            if !matches!(token.token, Token::WhiteSpace | Token::Comment) {
                break;
            }
            position = token.span.start;
        }
        Some(end - position)
    }

    fn should_have_after_white_space(&self, lexer: &CssParser<'s>) -> bool {
        let mut has_white_space = false;
        // The pseudo colon and name have already been consumed.
        let mut tokens = lexer.consumed_tokens().iter().rev().skip(2);
        let previous = loop {
            let Some(token) = tokens.next() else {
                return true;
            };
            match token.token {
                Token::Comment => {}
                Token::WhiteSpace => {
                    has_white_space = true;
                }
                _ => break *token,
            }
        };
        // start of a :global :local
        if matches!(
            previous.token,
            Token::ParenthesisBlock
                | Token::Comma
                | Token::Semicolon
                | Token::CloseCurlyBracket
                | Token::CurlyBracketBlock
                | Token::Function
        ) {
            return true;
        }
        has_white_space
    }

    fn eat(
        &mut self,
        lexer: &mut CssParser<'s>,
        kinds: &[Token],
        message: &'s str,
    ) -> Option<bool> {
        let token = lexer.current()?;
        if !kinds.contains(&token.token) {
            self.handle_warning.handle_warning(Warning {
                kind: WarningKind::Unexpected { message },
                range: Range::new(token.span.start, token.span.end),
            });
            return Some(false);
        }
        lexer.advance();
        Some(true)
    }

    fn lex_icss_import(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        let start = lexer.position();
        let end = if lexer.current_is(Token::CloseParenthesis) {
            start
        } else if lexer.current_is(Token::QuotedString) {
            lexer.advance()?.span.end
        } else {
            let mut end = start;
            while !lexer.current_is(Token::CloseParenthesis) {
                let token = lexer.advance()?;
                if !matches!(token.token, Token::WhiteSpace | Token::Comment) {
                    end = token.span.end;
                }
            }
            end
        };
        lexer.skip_trivia();
        while !lexer.current_is(Token::CloseParenthesis) {
            lexer.advance()?;
        }
        self.handle_dependency
            .handle_dependency(Dependency::ICSSImportFrom {
                path: lexer.slice(start, end)?,
            });
        lexer.advance();
        lexer.skip_trivia();
        if !self.eat(
            lexer,
            &[Token::CurlyBracketBlock],
            "Expected '{' during parsing of ':import()'",
        )? {
            return Some(());
        }
        lexer.skip_trivia();
        while !lexer.current_is(Token::CloseCurlyBracket) {
            lexer.skip_trivia();
            let prop_start = lexer.position();
            let prop_end = self.consume_icss_export_prop(lexer)?;
            lexer.skip_trivia();
            if !self.eat(
                lexer,
                &[Token::Colon],
                "Expected ':' during parsing of ':import'",
            )? {
                return Some(());
            }
            lexer.skip_trivia();
            let value_start = lexer.position();
            let value_end = self.consume_icss_import_value(lexer)?;
            if lexer.current_is(Token::Semicolon) {
                lexer.advance();
                lexer.skip_trivia();
            }
            self.handle_dependency
                .handle_dependency(Dependency::ICSSImportValue {
                    prop: lexer.slice(prop_start, prop_end)?,
                    value: lexer.slice(value_start, value_end)?,
                });
            self.icss_symbols
                .insert(escape::unescape(lexer.slice(prop_start, prop_end)?).into_owned());
        }
        lexer.advance();
        Some(())
    }

    fn consume_icss_export_prop(&self, lexer: &mut CssParser<'s>) -> Option<Pos> {
        let mut end = lexer.position();
        loop {
            let token = lexer.current()?;
            if matches!(
                token.token,
                Token::Colon | Token::CloseCurlyBracket | Token::Semicolon | Token::Comment
            ) {
                break;
            }
            lexer.advance();
            if token.token != Token::WhiteSpace {
                end = token.span.end;
            }
        }
        Some(end)
    }

    fn consume_icss_import_value(&self, lexer: &mut CssParser<'s>) -> Option<Pos> {
        let mut end = lexer.position();
        loop {
            let token = lexer.current()?;
            if matches!(token.token, Token::CloseCurlyBracket | Token::Semicolon) {
                break;
            }
            lexer.advance();
            if token.token != Token::WhiteSpace {
                end = token.span.end;
            }
        }
        Some(end)
    }

    fn consume_icss_export_value(&self, lexer: &mut CssParser<'s>) -> Option<Cow<'s, str>> {
        let start = lexer.position();
        let mut borrowed_end = start;
        let mut output = None::<String>;
        let mut pending_whitespace = None::<Range>;

        loop {
            let token = lexer.current()?;
            if matches!(token.token, Token::CloseCurlyBracket | Token::Semicolon) {
                break;
            }
            lexer.advance();
            match token.token {
                Token::WhiteSpace => {
                    if output.is_some() {
                        pending_whitespace = Some(Range::new(token.span.start, token.span.end));
                    }
                }
                Token::Comment => {
                    if let Some(output) = &mut output {
                        if let Some(range) = pending_whitespace.take() {
                            output.push_str(lexer.slice(range.start, range.end)?);
                        }
                    } else {
                        output = Some(lexer.slice(start, token.span.start)?.to_owned());
                    }
                    borrowed_end = token.span.end;
                }
                _ => {
                    if let Some(output) = &mut output {
                        if let Some(range) = pending_whitespace.take() {
                            output.push_str(lexer.slice(range.start, range.end)?);
                        }
                        output.push_str(lexer.raw(token));
                    }
                    borrowed_end = token.span.end;
                }
            }
        }

        Some(if let Some(output) = output {
            Cow::Owned(output)
        } else {
            Cow::Borrowed(lexer.slice(start, borrowed_end)?)
        })
    }

    fn consume_value_at_rule_params(&self, lexer: &mut CssParser<'s>) -> Option<(Pos, Pos)> {
        let start = lexer.position();
        loop {
            let token = lexer.current()?;
            if token.token == Token::Semicolon {
                let end = lexer.position();
                lexer.advance();
                return Some((start, end));
            }
            if token.token == Token::CloseCurlyBracket {
                return Some((start, lexer.position()));
            }
            lexer.advance();
        }
    }

    fn lex_value_at_rule(&mut self, lexer: &mut CssParser<'s>, start: Pos) -> Option<()> {
        let (params_start, params_end) = self.consume_value_at_rule_params(lexer)?;
        let at_rule_end = params_end.max(lexer.position());
        let params = lexer.slice(params_start, params_end)?;
        let tokens = lexer.rebased_tokens(params_start, params_end);
        match parse_value_at_rule_tokens(params, tokens) {
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
                        .handle_dependency(Dependency::ICSSImportFrom { path: from });
                    for item in items {
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSImportValue {
                                prop: item.local_name,
                                value: item.import_name,
                            });
                        self.icss_symbols
                            .insert(escape::unescape(item.local_name).into_owned());
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSExportValue {
                                prop: item.local_name,
                                value: Cow::Borrowed(item.local_name),
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
                            prop: local_name,
                            value: Cow::Borrowed(value),
                        });
                    self.icss_symbols
                        .insert(escape::unescape(local_name).into_owned());
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

    fn lex_icss_export(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        if !self.eat(
            lexer,
            &[Token::CurlyBracketBlock],
            "Expected '{' during parsing of ':export'",
        )? {
            return Some(());
        }
        lexer.skip_trivia();
        while !lexer.current_is(Token::CloseCurlyBracket) {
            lexer.skip_trivia();
            let prop_start = lexer.position();
            let prop_end = self.consume_icss_export_prop(lexer)?;
            lexer.skip_trivia();
            if !self.eat(
                lexer,
                &[Token::Colon],
                "Expected ':' during parsing of ':export'",
            )? {
                return Some(());
            }
            lexer.skip_trivia();
            let value = self.consume_icss_export_value(lexer)?;
            if lexer.current_is(Token::Semicolon) {
                lexer.advance();
                lexer.skip_trivia();
            }
            self.handle_dependency
                .handle_dependency(Dependency::ICSSExportValue {
                    prop: lexer.slice(prop_start, prop_end)?,
                    value,
                });
            self.icss_symbols
                .insert(escape::unescape(lexer.slice(prop_start, prop_end)?).into_owned());
        }
        lexer.advance();
        Some(())
    }

    fn lex_local_var(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        let token = lexer.current()?;
        let start = token.span.start;
        if token.token != Token::Ident || !lexer.decoded_token_name(token).starts_with("--") {
            self.handle_warning.handle_warning(Warning {
                kind: WarningKind::Unexpected {
                    message: "Expected starts with '--' during parsing of 'var()'",
                },
                range: Range::new(start, lexer.position_after_chars(2)),
            });
            return Some(());
        }
        lexer.advance();
        let prefix_len = escape::raw_prefix_len(lexer.raw_token_name(token), "--")? as Pos;
        let name_start = token.value_span.start + prefix_len;
        let end = token.value_span.end;
        lexer.skip_trivia();
        let from = if lexer.current_is_keyword("from") {
            lexer.advance();
            lexer.skip_trivia();
            let path = lexer.current()?;
            if matches!(path.token, Token::QuotedString | Token::Ident) {
                lexer.advance();
            } else {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(path.span.start, path.span.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected string or ident during parsing of 'composes'",
                    },
                });
                return Some(());
            }
            Some(lexer.raw(path))
        } else {
            None
        };
        self.handle_dependency
            .handle_dependency(Dependency::LocalVar {
                name: lexer.slice(name_start, end)?,
                range: Range::new(start, end),
                from,
            });
        Some(())
    }

    fn lex_local_var_decl(
        &mut self,
        lexer: &mut CssParser<'s>,
        name: &'s str,
        start: Pos,
        end: Pos,
    ) -> Option<()> {
        lexer.skip_trivia();
        if !lexer.current_is(Token::Colon) {
            return Some(());
        }
        lexer.advance();
        self.handle_dependency
            .handle_dependency(Dependency::LocalVarDecl {
                name,
                range: Range::new(start, end),
            });
        Some(())
    }

    fn lex_local_dashed_ident_decl(
        &mut self,
        lexer: &mut CssParser<'s>,
        local_decl_dependency: impl FnOnce(&'s str, Range) -> Dependency<'s>,
        dashed_warning: impl FnOnce(Range) -> Warning<'s>,
        left_curly_warning: impl FnOnce(Range) -> Warning<'s>,
    ) -> Option<()> {
        lexer.skip_trivia();
        let token = lexer.current()?;
        let start = token.span.start;
        if token.token != Token::Ident || !lexer.decoded_token_name(token).starts_with("--") {
            self.handle_warning
                .handle_warning(dashed_warning(Range::new(
                    start,
                    lexer.position_after_chars(2),
                )));
            return Some(());
        }
        lexer.advance();
        let prefix_len = escape::raw_prefix_len(lexer.raw_token_name(token), "--")? as Pos;
        let name_start = token.value_span.start + prefix_len;
        let end = token.value_span.end;
        self.handle_dependency
            .handle_dependency(local_decl_dependency(
                lexer.slice(name_start, end)?,
                Range::new(start, end),
            ));
        lexer.skip_trivia();
        if !lexer.current_is(Token::CurlyBracketBlock) {
            let token = lexer.current()?;
            self.handle_warning
                .handle_warning(left_curly_warning(Range::new(
                    token.span.start,
                    token.span.end,
                )));
            return Some(());
        }
        Some(())
    }

    fn lex_local_keyframes_decl(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        let mut is_function = false;
        if lexer.current_is(Token::Colon) {
            let start = lexer.position();
            lexer.parse_pseudo(self);
            let end = lexer.position();
            let pseudo = lexer.slice(start, end)?;
            let mode_data = self.mode_data.as_ref().unwrap();
            if mode_data.is_pure_mode()
                && !mode_data.is_pure_check_disabled()
                && (pseudo_name_eq(pseudo, "global", true)
                    || pseudo_name_eq(pseudo, "global", false))
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::NotPure {
                        message: "'@keyframes :global' is not allowed in pure mode",
                    },
                });
            }
            is_function =
                pseudo_name_eq(pseudo, "local", true) || pseudo_name_eq(pseudo, "global", true);
            if !is_function
                && !pseudo_name_eq(pseudo, "local", false)
                && !pseudo_name_eq(pseudo, "global", false)
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ':local', ':local()', ':global', or ':global()' during parsing of '@keyframes' name",
                    }
                });
                return Some(());
            }
            lexer.skip_trivia();
        }
        let start = lexer.position();
        let token = lexer.current()?;
        if token.token != Token::Ident {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, lexer.position_after_chars(2)),
                kind: WarningKind::Unexpected {
                    message: "Expected ident during parsing of '@keyframes' name",
                },
            });
            return Some(());
        }
        lexer.advance();
        let end = token.span.end;
        let mode_data = self.mode_data.as_mut().unwrap();
        if mode_data.is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframesDecl {
                    name: lexer.slice(start, end)?,
                    range: Range::new(start, end),
                });
        }
        lexer.skip_trivia();
        if is_function {
            if !lexer.current_is(Token::CloseParenthesis) {
                let token = lexer.current()?;
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(token.span.start, token.span.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected ')' during parsing of '@keyframes :local(' or '@keyframes :global('",
                    }
                });
                return Some(());
            }
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(lexer.current()?.span.start, lexer.current()?.span.end),
                });
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
            lexer.advance();
            lexer.skip_trivia();
        }
        if !lexer.current_is(Token::CurlyBracketBlock) {
            let token = lexer.current()?;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(token.span.start, token.span.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@keyframes'",
                },
            });
            return Some(());
        }
        Some(())
    }

    fn handle_local_keyframes_dependency(&mut self, lexer: &CssParser<'s>) -> Option<()> {
        let animation = self.in_animation_property.as_mut().unwrap();
        if let Some(range) = animation.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframes {
                    name: lexer.slice(range.start, range.end)?,
                    range,
                });
        }
        animation.reset_reserved();
        Some(())
    }

    fn lex_local_counter_style_decl(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        let start = lexer.position();
        let token = lexer.current()?;
        if token.token != Token::Ident {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, lexer.position_after_chars(2)),
                kind: WarningKind::Unexpected {
                    message: "Expected ident during parsing of '@counter-style'",
                },
            });
            return Some(());
        }
        lexer.advance();
        let end = token.span.end;
        self.handle_dependency
            .handle_dependency(Dependency::LocalCounterStyleDecl {
                name: lexer.slice(start, end)?,
                range: Range::new(start, end),
            });
        lexer.skip_trivia();
        if !lexer.current_is(Token::CurlyBracketBlock) {
            let token = lexer.current()?;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(token.span.start, token.span.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '{' during parsing of '@counter-style'",
                },
            });
            return Some(());
        }
        Some(())
    }

    fn lex_local_container_at_rule(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        if lexer.current_is(Token::ParenthesisBlock) {
            return Some(());
        }
        if !lexer.current_is(Token::Ident) {
            return Some(());
        }
        let start = lexer.position();
        let token = lexer.advance()?;
        let end = token.span.end;
        let ident = lexer.raw(token);
        if is_reserved_container_query_ident(ident) {
            return Some(());
        }
        if self.mode_data.as_ref().unwrap().is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalContainer {
                    name: ident,
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn lex_local_function_decl(&mut self, lexer: &mut CssParser<'s>) -> Option<()> {
        lexer.skip_trivia();
        let mut is_function = false;
        if lexer.current_is(Token::Colon) {
            let start = lexer.position();
            lexer.parse_pseudo(self);
            let end = lexer.position();
            let pseudo = lexer.slice(start, end)?;
            let mode_data = self.mode_data.as_ref().unwrap();
            if mode_data.is_pure_mode()
                && !mode_data.is_pure_check_disabled()
                && (pseudo_name_eq(pseudo, "global", true)
                    || pseudo_name_eq(pseudo, "global", false))
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::NotPure {
                        message: "'@function :global' is not allowed in pure mode",
                    },
                });
            }
            is_function =
                pseudo_name_eq(pseudo, "local", true) || pseudo_name_eq(pseudo, "global", true);
            if !is_function
                && !pseudo_name_eq(pseudo, "local", false)
                && !pseudo_name_eq(pseudo, "global", false)
            {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::Unexpected {
                        message:
                            "Expected ':local', ':local()', ':global', or ':global()' during parsing of '@function' name",
                    },
                });
                return Some(());
            }
            lexer.skip_trivia();
        }
        let start = lexer.position();
        let token = lexer.current()?;
        let name_is_function = token.token == Token::Function;
        if !matches!(token.token, Token::Ident | Token::Function)
            || !lexer.decoded_token_name(token).starts_with("--")
        {
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, lexer.position_after_chars(2)),
                kind: WarningKind::Unexpected {
                    message: "Expected starts with '--' during parsing of '@function' name",
                },
            });
            return Some(());
        }
        lexer.advance();
        let prefix_len = escape::raw_prefix_len(lexer.raw_token_name(token), "--")? as Pos;
        let name_start = token.value_span.start + prefix_len;
        let end = token.value_span.end;
        let mode_data = self.mode_data.as_mut().unwrap();
        if mode_data.is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalFunctionDecl {
                    name: lexer.slice(name_start, end)?,
                    range: Range::new(start, end),
                });
        }
        if name_is_function {
            self.balanced.push(
                BalancedItem::new_other(end, token.span.end),
                Some(mode_data),
            );
        }
        lexer.skip_trivia();
        if is_function {
            if !lexer.current_is(Token::CloseParenthesis) {
                let token = lexer.current()?;
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(token.span.start, token.span.end),
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
                    range: Range::new(lexer.current()?.span.start, lexer.current()?.span.end),
                });
            mode_data.inside_mode_function -= 1;
            self.balanced.pop_without_moda_data();
            lexer.advance();
            lexer.skip_trivia();
        }
        if !name_is_function && !lexer.current_is(Token::ParenthesisBlock) {
            let token = lexer.current()?;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(token.span.start, token.span.end),
                kind: WarningKind::Unexpected {
                    message: "Expected '(' during parsing of '@function'",
                },
            });
        }
        Some(())
    }

    fn handle_local_counter_style_dependency(&mut self, lexer: &CssParser<'s>) -> Option<()> {
        let list_style = self.in_list_style_property.as_mut().unwrap();
        if let Some(range) = list_style.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalCounterStyle {
                    name: lexer.slice(range.start, range.end)?,
                    range,
                });
        }
        Some(())
    }

    fn handle_local_font_palette_dependency(&mut self, lexer: &CssParser<'s>) -> Option<()> {
        let font_palette = self.in_font_palette_property.as_mut().unwrap();
        if let Some(range) = font_palette.take_rename(self.balanced.len()) {
            self.handle_dependency
                .handle_dependency(Dependency::LocalFontPalette {
                    name: lexer.slice(range.start + 2, range.end)?,
                    range,
                });
        }
        Some(())
    }

    fn lex_composes(
        &mut self,
        lexer: &mut CssParser<'s>,
        local_classes: SmallVec<[&'s str; 2]>,
        start: Pos,
    ) -> Option<()> {
        lexer.skip_trivia();
        if !lexer.current_is(Token::Colon) {
            return Some(());
        }
        lexer.advance();
        let mut names: SmallVec<[&'s str; 2]> = SmallVec::new();
        let mut end;
        loop {
            let mut has_from = false;
            lexer.skip_trivia();
            let start = lexer.position();
            end = start;
            loop {
                let token = lexer.current()?;
                if matches!(
                    token.token,
                    Token::Comma | Token::Semicolon | Token::CloseCurlyBracket
                ) {
                    break;
                }
                let maybe_global_start = lexer.position();
                if token.token == Token::Function && lexer.token_is_keyword(token, "global") {
                    lexer.advance();
                    let name_start = lexer.position();
                    let name = lexer.current()?;
                    if name.token != Token::Ident {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(name_start, name.span.end),
                            kind: WarningKind::Unexpected {
                                message: "Expected ident during parsing of 'composes'",
                            },
                        });
                        return Some(());
                    }
                    lexer.advance();
                    let name_end = name.span.end;
                    lexer.skip_trivia();
                    self.eat(
                        lexer,
                        &[Token::CloseParenthesis],
                        "Expected ')' during parsing of 'composes'",
                    );
                    end = lexer.position();
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: local_classes.clone(),
                            names: smallvec![lexer.slice(name_start, name_end)?],
                            from: Some("global"),
                            range: Range::new(maybe_global_start, lexer.position()),
                        });
                } else {
                    let name_start = lexer.position();
                    if token.token != Token::Ident {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(name_start, token.span.end),
                            kind: WarningKind::Unexpected {
                                message: "Expected ident during parsing of 'composes'",
                            },
                        });
                        return Some(());
                    }
                    lexer.advance();
                    let name_end = token.span.end;
                    let ident = lexer.raw(token);
                    if !names.is_empty() && lexer.token_is_keyword(token, "from") {
                        lexer.skip_trivia();
                        if matches!(
                            lexer.current().map(|token| token.token),
                            Some(Token::QuotedString | Token::Ident)
                        ) {
                            has_from = true;
                            break;
                        }
                    }
                    names.push(ident);
                    end = name_end;
                }
                lexer.skip_trivia();
            }
            lexer.skip_trivia();
            if !has_from {
                if !names.is_empty() {
                    self.handle_dependency
                        .handle_dependency(Dependency::Composes {
                            local_classes: local_classes.clone(),
                            names: std::mem::take(&mut names),
                            from: None,
                            range: Range::new(start, end),
                        });
                }
                if lexer.current_is(Token::Comma) {
                    lexer.advance();
                    continue;
                }
                break;
            }
            let path = lexer.current()?;
            if matches!(path.token, Token::QuotedString | Token::Ident) {
                lexer.advance();
            } else {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(path.span.start, path.span.end),
                    kind: WarningKind::Unexpected {
                        message: "Expected string or ident during parsing of 'composes'",
                    },
                });
                return Some(());
            }
            let path_start = path.span.start;
            let path_end = path.span.end;
            end = path_end;
            let from = Some(lexer.slice(path_start, path_end)?);
            self.handle_dependency
                .handle_dependency(Dependency::Composes {
                    local_classes: local_classes.clone(),
                    names: std::mem::take(&mut names),
                    from,
                    range: Range::new(start, end),
                });
            lexer.skip_trivia();
            if !lexer.current_is(Token::Comma) {
                break;
            }
            lexer.advance();
        }
        if lexer.current_is(Token::Semicolon) {
            lexer.advance();
            end = lexer.position();
        }
        self.handle_dependency
            .handle_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, end),
            });
        Some(())
    }
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> TokenVisitor<'s>
    for LexDependencies<'s, D, W>
{
    fn comment(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
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

    fn is_selector(&mut self, lexer: &mut CssParser<'s>) -> Option<bool> {
        if self.is_next_rule_prelude || self.non_selector_mode.is_some() {
            return Some(true);
        }
        self.non_selector_mode = self.get_non_selector_mode_pseudo(lexer)?;
        Some(self.non_selector_mode.is_some())
    }

    fn url(
        &mut self,
        lexer: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        content_start: Pos,
        content_end: Pos,
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
                import_data.url_range = Some(Range::new(start, end));
            }
            Scope::InBlock => self.handle_dependency.handle_dependency(Dependency::Url {
                request: value,
                range: Range::new(start, end),
                kind: UrlRangeKind::Function,
            }),
            _ => {}
        }
        Some(())
    }

    fn string(
        &mut self,
        lexer: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        value_start: Pos,
        value_end: Pos,
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

                let value = lexer.slice(value_start, value_end)?;
                import_data.url = Some(value);
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
                        for range in grid_template_area_ranges(
                            lexer.slice(value_start, value_end)?,
                            value_start,
                        ) {
                            self.handle_dependency
                                .handle_dependency(Dependency::LocalGridDecl {
                                    name: lexer.slice(range.start, range.end)?,
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
                let value = lexer.slice(value_start, value_end)?;
                self.handle_dependency.handle_dependency(Dependency::Url {
                    request: value,
                    range: Range::new(start, end),
                    kind,
                });
            }
            _ => {}
        }
        Some(())
    }

    fn at_keyword(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?;
        if at_keyword_name_eq(name, "namespace") {
            self.scope = Scope::AtNamespaceInvalid;
            self.handle_warning.handle_warning(Warning {
                range: Range::new(start, end),
                kind: WarningKind::NamespaceNotSupportedInBundledCss,
            });
        } else if at_keyword_name_eq(name, "import") {
            if !self.allow_import_at_rule {
                self.scope = Scope::AtImportInvalid;
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::NotPrecededAtImport,
                });
                return Some(());
            }
            self.scope = Scope::InAtImport(ImportData::new(start));
        } else if at_keyword_name_eq(name, "charset") {
            self.lex_charset_at_rule(lexer, start)?;
        } else if self.mode_data.is_some() {
            let mut can_contain_rules = true;
            if at_keyword_name_eq(name, "value") {
                self.lex_value_at_rule(lexer, start)?;
                can_contain_rules = false;
            } else if at_keyword_name_eq(name, "keyframes")
                || with_vendor_prefixed_eq(name, "keyframes", true)
            {
                self.lex_local_keyframes_decl(lexer)?;
            } else if at_keyword_name_eq(name, "container") {
                self.lex_local_container_at_rule(lexer)?;
            } else if at_keyword_name_eq(name, "function") {
                self.lex_local_function_decl(lexer)?;
            } else if at_keyword_name_eq(name, "property") {
                self.lex_local_dashed_ident_decl(
                    lexer,
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
            } else if at_keyword_name_eq(name, "counter-style") {
                self.lex_local_counter_style_decl(lexer)?;
            } else if at_keyword_name_eq(name, "font-palette-values") {
                self.lex_local_dashed_ident_decl(
                    lexer,
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
                self.is_next_rule_prelude = at_keyword_name_eq(name, "scope");
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

    fn semicolon(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        match self.scope {
            Scope::InAtImport(ref import_data) => {
                let Some(url) = import_data.url else {
                    if let Some((name, name_range)) = import_data.prelude.icss_import_url() {
                        self.handle_dependency
                            .handle_dependency(Dependency::ICSSImportUrl {
                                name,
                                range: Range::new(import_data.start, end),
                                name_range: name_range.clone(),
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
                        range: url_range.clone(),
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
                                range: url_range.clone(),
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
                                range: url_range.clone(),
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
                                range: layer_range.clone(),
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
                self.non_selector_mode = None;
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
            }
            Scope::TopLevel => {
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
            }
        }
        Some(())
    }

    fn function(
        &mut self,
        lexer: &mut CssParser<'s>,
        start: Pos,
        end: Pos,
        value_start: Pos,
        value_end: Pos,
    ) -> Option<()> {
        let name = lexer.slice(start, end)?;
        let item = BalancedItem::new(name, start, end);
        let at_import_top_level =
            matches!(self.scope, Scope::InAtImport(_)) && self.balanced.is_empty();
        self.balanced.push(item, self.mode_data.as_mut());

        if let Scope::InAtImport(ref mut import_data) = self.scope {
            if at_import_top_level && function_name_eq(name, "url") {
                import_data.prelude.push(ImportPreludeNode::Url {
                    range: Range::new(start, end),
                });
            } else if at_import_top_level && function_name_eq(name, "layer") {
                import_data.prelude.push(ImportPreludeNode::Layer {
                    range: Range::new(start, end),
                });
            } else if at_import_top_level && function_name_eq(name, "supports") {
                import_data.prelude.push(ImportPreludeNode::Supports {
                    range: Range::new(start, end),
                });
                import_data.supports = ImportDataSupports::InSupports;
            } else if at_import_top_level {
                import_data.prelude.push(ImportPreludeNode::Other {
                    range: Range::new(start, end),
                });
            } else if function_name_eq(name, "supports") {
                import_data.supports = ImportDataSupports::InSupports;
            }
        }

        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        let raw_token_name = lexer.slice(value_start, value_end)?;
        let token_name = escape::unescape(raw_token_name);
        if mode_data.is_current_local_mode() && token_name.starts_with("--") {
            let prefix_len = escape::raw_prefix_len(raw_token_name, "--")? as Pos;
            self.handle_dependency
                .handle_dependency(Dependency::LocalFunction {
                    name: lexer.slice(value_start + prefix_len, value_end)?,
                    range: Range::new(start, value_end),
                });
        }
        if mode_data.is_current_local_mode() && function_name_eq(name, "var") {
            self.lex_local_var(lexer)?;
        }
        Some(())
    }

    fn left_parenthesis(&mut self, _: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        self.balanced
            .push(BalancedItem::new_other(start, end), self.mode_data.as_mut());
        Some(())
    }

    fn right_parenthesis(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
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
                let distance = self.back_white_space_and_comments_distance(lexer, start)?;
                let start = start - distance;
                let maybe_left_parenthesis_start = start - 1;
                if lexer.slice(start - 1, start)? == "(" {
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
                        range: Range::new(start, end),
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

    fn ident(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        match self.scope {
            Scope::InBlock => {
                let Some(mode_data) = &mut self.mode_data else {
                    return Some(());
                };

                let ident = lexer.slice(start, end)?;
                if self.icss_symbols.contains(escape::unescape(ident).as_ref()) {
                    self.handle_dependency
                        .handle_dependency(Dependency::ICSSSymbol {
                            name: ident,
                            range: Range::new(start, end),
                        });
                    return Some(());
                }
                if mode_data.is_current_local_mode()
                    && self
                        .balanced
                        .last()
                        .map(|last| last.kind.is_mode_function())
                        .unwrap_or(false)
                    && escape::unescape(ident).starts_with("--")
                    && lexer.current_is(Token::CloseParenthesis)
                {
                    let prefix_len = escape::raw_prefix_len(ident, "--")? as Pos;
                    self.handle_dependency
                        .handle_dependency(Dependency::LocalFunction {
                            name: lexer.slice(start + prefix_len, end)?,
                            range: Range::new(start, end),
                        });
                    return Some(());
                }
                if mode_data.is_property_local_mode() {
                    if let Some(animation) = &mut self.in_animation_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            animation.set_rename(lexer.slice(start, end)?, Range::new(start, end));
                        }
                        return Some(());
                    }

                    if let Some(list_style) = &mut self.in_list_style_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            list_style.set_rename(lexer.slice(start, end)?, Range::new(start, end));
                        }
                        return Some(());
                    }

                    if let Some(font_palette) = &mut self.in_font_palette_property {
                        // Not inside functions or inside palette-mix()
                        if self.balanced.is_empty()
                            || matches!(self.balanced.last(), Some(last) if matches!(last.kind, BalancedItemKind::PaletteMix))
                        {
                            font_palette
                                .set_rename(lexer.slice(start, end)?, Range::new(start, end));
                        }
                        return Some(());
                    }

                    if let Some(container) = &mut self.in_container_property {
                        if self.balanced.is_empty() {
                            container.set_rename(ident, Range::new(start, end));
                            if let Some(range) = container.take_rename(self.balanced.len()) {
                                self.handle_dependency.handle_dependency(
                                    Dependency::LocalContainerDecl {
                                        name: lexer.slice(range.start, range.end)?,
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
                                    name: ident,
                                    range: Range::new(start, end),
                                });
                        }
                        return Some(());
                    }

                    if let Some(prefix_len) = escape::raw_prefix_len(ident, "--") {
                        return self.lex_local_var_decl(
                            lexer,
                            lexer.slice(start + prefix_len as Pos, end)?,
                            start,
                            end,
                        );
                    }

                    if escape::eq_ignore_ascii_case(ident, "animation")
                        || escape::eq_ignore_ascii_case(ident, "animation-name")
                        || with_vendor_prefixed_eq(ident, "animation", false)
                        || with_vendor_prefixed_eq(ident, "animation-name", false)
                    {
                        self.enter_animation_property();
                        return Some(());
                    }

                    if escape::eq_ignore_ascii_case(ident, "list-style")
                        || escape::eq_ignore_ascii_case(ident, "list-style-type")
                    {
                        self.enter_list_style_property();
                        return Some(());
                    }

                    if escape::eq_ignore_ascii_case(ident, "font-palette") {
                        self.enter_font_palette_property();
                        return Some(());
                    }

                    if escape::eq_ignore_ascii_case(ident, "container")
                        || escape::eq_ignore_ascii_case(ident, "container-name")
                    {
                        self.enter_container_property();
                        return Some(());
                    }

                    if let Some(kind) = grid_property_kind(ident) {
                        self.enter_grid_property(kind);
                        return Some(());
                    }
                }

                if escape::eq_ignore_ascii_case(ident, "composes")
                    || escape::eq_ignore_ascii_case(ident, "compose-with")
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
                        .get_valid_local_classes(lexer)
                    else {
                        self.handle_warning.handle_warning(Warning {
                            range: Range::new(start, end),
                            kind: WarningKind::UnexpectedComposition {
                                message: "only allowed when selector is single :local class",
                            },
                        });
                        return Some(());
                    };
                    return self.lex_composes(lexer, local_classes, start);
                }
            }
            Scope::InAtImport(ref mut import_data) => {
                if !self.balanced.is_empty() || import_data.in_supports() {
                    return Some(());
                }

                let ident = lexer.slice(start, end)?;
                if escape::eq_ignore_ascii_case(ident, "layer") {
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

    fn class(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice(start, end)?;
        if let Some(mode) = self.non_selector_mode.take() {
            if name != "." && mode == Mode::Local {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalClass {
                        name,
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
                    name,
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

    fn id(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice(start, end)?;
        if let Some(mode) = self.non_selector_mode.take() {
            if name != "#" && mode == Mode::Local {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalId {
                        name,
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
                    name,
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

    fn left_curly_bracket(&mut self, lexer: &mut CssParser<'s>, start: Pos, _: Pos) -> Option<()> {
        self.non_selector_mode = None;
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
        self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
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

    fn right_curly_bracket(&mut self, lexer: &mut CssParser<'s>, _: Pos, end: Pos) -> Option<()> {
        self.non_selector_mode = None;
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
                self.is_next_rule_prelude = true;
                if let Some(mode_data) = &mut self.mode_data {
                    mode_data.composes_local_classes.reset_to_initial();
                }
            } else {
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
            }
        }
        Some(())
    }

    fn pseudo_function(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?;
        if let Some(mode_data) = &mut self.mode_data {
            if pseudo_name_eq(name, "import", true) {
                self.lex_icss_import(lexer);
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start, lexer.position()),
                    });
                return Some(());
            }
            if pseudo_name_eq(name, "global", true) || pseudo_name_eq(name, "local", true) {
                if mode_data.is_inside_mode_function() {
                    self.handle_warning.handle_warning(Warning {
                        range: Range::new(start, end),
                        kind: WarningKind::ExpectedNotInside {
                            pseudo: lexer.slice(start, end)?,
                        },
                    });
                }

                lexer.skip_trivia();
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start, lexer.position()),
                    });
            } else if self.block_nesting_level == 0 {
                mode_data.composes_local_classes.invalidate();
            }
        }
        self.balanced
            .push(BalancedItem::new(name, start, end), self.mode_data.as_mut());
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        let name = lexer.slice(start, end)?;
        if pseudo_name_eq(name, "global", false) || pseudo_name_eq(name, "local", false) {
            let is_non_selector_mode = self.non_selector_mode.is_some();
            if !is_non_selector_mode && mode_data.is_inside_mode_function() {
                self.handle_warning.handle_warning(Warning {
                    range: Range::new(start, end),
                    kind: WarningKind::ExpectedNotInside {
                        pseudo: lexer.slice(start, end)?,
                    },
                });
            }

            let should_have_after_white_space = self.should_have_after_white_space(lexer);
            let trivia = lexer.consume_trivia();
            let has_after_white_space = trivia.has_white_space;
            let has_trailing_trivia = trivia.end != end;
            let kind = lexer.current()?.token;
            if kind != Token::CloseParenthesis
                && kind != Token::Comma
                && (kind != Token::CurlyBracketBlock
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
                    range: Range::new(start, trivia.first_comment_start.unwrap_or(trivia.end)),
                });
            return Some(());
        }
        if matches!(self.scope, Scope::TopLevel) && pseudo_name_eq(name, "export", false) {
            self.lex_icss_export(lexer)?;
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, lexer.position()),
                });
            return Some(());
        }

        if self.block_nesting_level == 0 {
            mode_data.composes_local_classes.invalidate();
        }
        Some(())
    }

    fn comma(&mut self, lexer: &mut CssParser<'s>, start: Pos, end: Pos) -> Option<()> {
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
