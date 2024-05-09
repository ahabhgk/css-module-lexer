use std::fmt::Display;

use smallvec::SmallVec;

use crate::lexer::is_white_space;
use crate::lexer::start_ident_sequence;
use crate::lexer::C_ASTERISK;
use crate::lexer::C_COLON;
use crate::lexer::C_COMMA;
use crate::lexer::C_HYPHEN_MINUS;
use crate::lexer::C_LEFT_CURLY;
use crate::lexer::C_LEFT_PARENTHESIS;
use crate::lexer::C_RIGHT_CURLY;
use crate::lexer::C_RIGHT_PARENTHESIS;
use crate::lexer::C_SEMICOLON;
use crate::lexer::C_SOLIDUS;
use crate::HandleDependency;
use crate::HandleWarning;
use crate::Lexer;
use crate::Pos;
use crate::Visitor;

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
    url: Option<&'s str>,
    url_range: Option<Range>,
    supports: ImportDataSupports<'s>,
    layer: ImportDataLayer<'s>,
}

impl ImportData<'_> {
    pub fn new(start: Pos) -> Self {
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
            if matches!(
                item.kind,
                BalancedItemKind::LocalFn | BalancedItemKind::LocalClass
            ) {
                mode_data.set_current_mode(Mode::Local);
            } else if matches!(
                item.kind,
                BalancedItemKind::GlobalFn | BalancedItemKind::GlobalClass
            ) {
                mode_data.set_current_mode(Mode::Global);
            }
        }
        self.0.push(item);
    }

    pub fn pop(&mut self, mode_data: Option<&mut ModeData>) -> Option<BalancedItem> {
        let item = self.0.pop()?;
        if let Some(mode_data) = mode_data {
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

    pub fn inside_mode_function(&self) -> Option<Pos> {
        let mut iter = self.0.iter();
        loop {
            if let Some(last) = iter.next_back() {
                if matches!(
                    last.kind,
                    BalancedItemKind::LocalFn | BalancedItemKind::GlobalFn
                ) {
                    return Some(last.range.start);
                }
            } else {
                return None;
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
}

#[derive(Debug)]
enum BalancedItemKind {
    Url,
    ImageSet,
    Layer,
    Supports,
    LocalFn,
    GlobalFn,
    LocalClass,
    GlobalClass,
    Other,
}

impl BalancedItemKind {
    pub fn new(name: &str) -> Self {
        match name {
            "url(" => Self::Url,
            "image-set(" => Self::ImageSet,
            _ if with_vendor_prefixed_eq(name, "image-set(") => Self::ImageSet,
            "layer(" => Self::Layer,
            "supports(" => Self::Supports,
            ":local(" => Self::LocalFn,
            ":global(" => Self::GlobalFn,
            ":local" => Self::LocalClass,
            ":global" => Self::GlobalClass,
            _ => Self::Other,
        }
    }
}

fn with_vendor_prefixed_eq(left: &str, right: &str) -> bool {
    left.strip_prefix("-webkit-") == Some(right)
        || left.strip_prefix("-moz-") == Some(right)
        || left.strip_prefix("-ms-") == Some(right)
        || left.strip_prefix("-o-") == Some(right)
}

fn with_at_vendor_prefixed_eq(left: &str, right: &str) -> bool {
    if let Some(left) = left.strip_prefix('@') {
        return with_vendor_prefixed_eq(left, right);
    }
    false
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

#[derive(Debug, Default, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Local,
    Global,
    Pure,
}

#[derive(Debug)]
pub struct ModeData {
    default: Mode,
    current: Mode,
    property: Mode,
    resulting_global: Option<Pos>,
    pure_global: Option<Pos>,
}

impl ModeData {
    pub fn new(default: Mode) -> Self {
        Self {
            default,
            current: default,
            property: default,
            resulting_global: None,
            pure_global: Some(0),
        }
    }

    pub fn is_pure_mode(&self) -> bool {
        matches!(self.default, Mode::Pure)
    }

    pub fn is_current_local_mode(&self) -> bool {
        match self.current {
            Mode::Local | Mode::Pure => true,
            Mode::Global => false,
        }
    }

    pub fn is_property_local_mode(&self) -> bool {
        match self.property {
            Mode::Local | Mode::Pure => true,
            Mode::Global => false,
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
}

#[derive(Debug)]
struct AnimationProperty {
    keywords: AnimationKeywords,
    keyframes: Option<Range>,
    balanced_len: usize,
}

impl AnimationProperty {
    pub fn new(balanced_len: usize) -> Self {
        Self {
            keywords: AnimationKeywords::default(),
            keyframes: None,
            balanced_len,
        }
    }

    fn check_keywords(&mut self, ident: &str) -> bool {
        self.keywords.check(ident)
    }

    pub fn reset_keywords(&mut self) {
        self.keywords.reset();
    }

    pub fn set_keyframes(&mut self, ident: &str, range: Range) {
        if self.check_keywords(ident) {
            self.keyframes = Some(range);
        }
    }

    pub fn take_keyframes(&mut self, balanced_len: usize) -> Option<Range> {
        // Don't rename animation name when we in functions
        if balanced_len != self.balanced_len {
            return None;
        }
        std::mem::take(&mut self.keyframes)
    }
}

#[derive(Debug, Default)]
struct AnimationKeywords {
    bits: u32,
}

impl AnimationKeywords {
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

    pub fn check(&mut self, ident: &str) -> bool {
        match ident {
            "normal" => self.keyword_check(Self::NORMAL),
            "reverse" => self.keyword_check(Self::REVERSE),
            "alternate" => self.keyword_check(Self::ALTERNATE),
            "alternate-reverse" => self.keyword_check(Self::ALTERNATE_REVERSE),
            "forwards" => self.keyword_check(Self::FORWARDS),
            "backwards" => self.keyword_check(Self::BACKWARDS),
            "both" => self.keyword_check(Self::BOTH),
            "infinite" => self.keyword_check(Self::INFINITE),
            "paused" => self.keyword_check(Self::PAUSED),
            "running" => self.keyword_check(Self::RUNNING),
            "ease" => self.keyword_check(Self::EASE),
            "ease-in" => self.keyword_check(Self::EASE_IN),
            "ease-out" => self.keyword_check(Self::EASE_OUT),
            "ease-in-out" => self.keyword_check(Self::EASE_IN_OUT),
            "linear" => self.keyword_check(Self::LINEAR),
            "step-end" => self.keyword_check(Self::STEP_END),
            "step-start" => self.keyword_check(Self::STEP_START),
            "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" => false,
            _ => true,
        }
    }

    fn keyword_check(&mut self, bit: u32) -> bool {
        if self.bits & bit == bit {
            return true;
        }
        self.bits |= bit;
        false
    }

    pub fn reset(&mut self) {
        self.bits = 0;
    }
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
    Replace {
        content: &'s str,
        range: Range,
    },
    LocalIdent {
        name: &'s str,
        range: Range,
    },
    LocalVar {
        name: &'s str,
        range: Range,
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
    Composes {
        names: &'s str,
        from: Option<&'s str>,
    },
    ICSSExport {
        prop: &'s str,
        value: &'s str,
    },
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum UrlRangeKind {
    Function,
    String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Warning<'s> {
    Unexpected { range: Range, message: &'s str },
    DuplicateUrl { range: Range, when: &'s str },
    NamespaceNotSupportedInBundledCss { range: Range },
    NotPrecededAtImport { range: Range },
    ExpectedUrl { range: Range, when: &'s str },
    ExpectedUrlBefore { range: Range, when: &'s str },
    ExpectedLayerBefore { range: Range, when: &'s str },
    InconsistentModeResult { range: Range },
    ExpectedNotInside { range: Range, pseudo: &'s str },
    MissingWhitespace { range: Range, surrounding: &'s str },
    NotPure { range: Range, message: &'s str },
}

impl Display for Warning<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Warning::Unexpected { message, .. } => write!(f, "{message}"),
            Warning::DuplicateUrl { when, .. } => write!(
                f,
                "Duplicate of 'url(...)' in '{when}'"
            ),
            Warning::NamespaceNotSupportedInBundledCss { .. } => write!(
                f,
                "'@namespace' is not supported in bundled CSS"
            ),
            Warning::NotPrecededAtImport { .. } => {
                write!(f, "Any '@import' rules must precede all other rules")
            }
            Warning::ExpectedUrl { when, .. } => write!(f, "Expected URL in '{when}'"),
            Warning::ExpectedUrlBefore { when, .. } => write!(
                f,
                "An URL in '{when}' should be before 'layer(...)' or 'supports(...)'"
            ),
            Warning::ExpectedLayerBefore { when, .. } => write!(
                f,
                "The 'layer(...)' in '{when}' should be before 'supports(...)'"
            ),
            Warning::InconsistentModeResult { .. } => write!(
                f,
                "Inconsistent rule global/local (multiple selectors must result in the same mode for the rule)"
            ),
            Warning::ExpectedNotInside { pseudo, .. } => write!(
                f,
                "A '{pseudo}' is not allowed inside of a ':local()' or ':global()'"
            ),
            Warning::MissingWhitespace { surrounding, .. } => write!(
                f,
                "Missing {surrounding} whitespace"
            ),
            Warning::NotPure { message, .. } => write!(f, "Pure globals is not allowed in pure mode, {message}"),
        }
    }
}

#[derive(Debug)]
pub struct LexDependencies<'s, D, W> {
    mode_data: Option<ModeData>,
    scope: Scope<'s>,
    block_nesting_level: u32,
    allow_import_at_rule: bool,
    balanced: BalancedStack,
    is_next_rule_prelude: bool,
    in_animation_property: Option<AnimationProperty>,
    handle_dependency: D,
    handle_warning: W,
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> LexDependencies<'s, D, W> {
    pub fn new(handle_dependency: D, handle_warning: W, mode_data: Option<ModeData>) -> Self {
        Self {
            mode_data,
            scope: Scope::TopLevel,
            block_nesting_level: 0,
            allow_import_at_rule: true,
            balanced: Default::default(),
            is_next_rule_prelude: true,
            in_animation_property: None,
            handle_dependency,
            handle_warning,
        }
    }

    fn is_next_nested_syntax(&self, lexer: &mut Lexer) -> Option<bool> {
        lexer.consume_white_space_and_comments()?;
        let c = lexer.cur()?;
        if c == C_RIGHT_CURLY {
            return Some(false);
        }
        // If what follows is a property, then it's not a nested selector
        // This is not strictly correct, but it's good enough for our purposes
        // since we only need 'is_selector()' when next char is '#', '.', or ':'
        Some(!start_ident_sequence(c, lexer.peek()?, lexer.peek2()?))
    }

    fn get_media(&self, lexer: &Lexer<'s>, start: Pos, end: Pos) -> Option<&'s str> {
        let media = lexer.slice(start, end)?;
        let mut media_lexer = Lexer::from(media);
        media_lexer.consume();
        media_lexer.consume_white_space_and_comments()?;
        Some(media)
    }

    fn enter_animation_property(&mut self) {
        self.in_animation_property = Some(AnimationProperty::new(self.balanced.len()));
    }

    fn exit_animation_property(&mut self) {
        self.in_animation_property = None;
    }

    fn back_white_space_and_comments_distance(&self, lexer: &Lexer<'s>, end: Pos) -> Option<Pos> {
        let mut lexer = lexer.clone().turn_back(end);
        lexer.consume();
        lexer.consume_white_space_and_comments()?;
        lexer.cur_pos()
    }

    fn should_have_after_white_space(&self, lexer: &Lexer<'s>, end: Pos) -> bool {
        let mut lexer = lexer.clone().turn_back(end);
        let mut has_white_space = false;
        lexer.consume();
        loop {
            if lexer.consume_comments().is_none() {
                return true;
            }
            let Some(c) = lexer.cur() else {
                return true;
            };
            if is_white_space(c) {
                has_white_space = true;
                if lexer.consume_space().is_none() {
                    return true;
                }
            } else {
                break;
            }
        }
        let c = lexer.cur().unwrap();
        // start of a :global :local
        if c == C_LEFT_PARENTHESIS || c == C_COMMA || c == C_SEMICOLON || c == C_RIGHT_CURLY {
            return true;
        }
        has_white_space
    }

    fn has_after_white_space(&self, lexer: &mut Lexer<'s>) -> Option<bool> {
        let mut has_white_space = false;
        loop {
            lexer.consume_comments()?;
            if is_white_space(lexer.cur()?) {
                has_white_space = true;
                lexer.consume_space()?;
            } else {
                break;
            }
        }
        Some(has_white_space)
    }

    fn consume_icss_export_prop(&self, lexer: &mut Lexer<'s>) -> Option<()> {
        loop {
            let c = lexer.cur()?;
            if c == C_COLON
                || c == C_RIGHT_CURLY
                || c == C_SEMICOLON
                || (c == C_SOLIDUS && lexer.peek()? == C_ASTERISK)
            {
                break;
            }
            lexer.consume();
        }
        Some(())
    }

    fn consume_icss_export_value(&self, lexer: &mut Lexer<'s>) -> Option<()> {
        loop {
            let c = lexer.cur()?;
            if c == C_RIGHT_CURLY || c == C_SEMICOLON {
                break;
            }
            lexer.consume();
        }
        Some(())
    }

    fn lex_icss_export(&mut self, lexer: &mut Lexer<'s>) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        let c = lexer.cur()?;
        if c != C_LEFT_CURLY {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected '{' during parsing of ':export'",
                range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
            });
            return Some(());
        }
        lexer.consume();
        lexer.consume_white_space_and_comments()?;
        while lexer.cur()? != C_RIGHT_CURLY {
            lexer.consume_white_space_and_comments()?;
            let prop_start = lexer.cur_pos()?;
            self.consume_icss_export_prop(lexer)?;
            let prop_end = lexer.cur_pos()?;
            lexer.consume_white_space_and_comments()?;
            if lexer.cur()? != C_COLON {
                self.handle_warning.handle_warning(Warning::Unexpected {
                    message: "Expected ':' during parsing of ':export'",
                    range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
                });
                return Some(());
            }
            lexer.consume();
            lexer.consume_white_space_and_comments()?;
            let value_start = lexer.cur_pos()?;
            self.consume_icss_export_value(lexer)?;
            let value_end = lexer.cur_pos()?;
            if lexer.cur()? == C_SEMICOLON {
                lexer.consume();
                lexer.consume_white_space_and_comments()?;
            }
            self.handle_dependency
                .handle_dependency(Dependency::ICSSExport {
                    prop: lexer
                        .slice(prop_start, prop_end)?
                        .trim_end_matches(is_white_space),
                    value: lexer
                        .slice(value_start, value_end)?
                        .trim_end_matches(is_white_space),
                });
        }
        lexer.consume();
        Some(())
    }

    fn lex_local_var(&mut self, lexer: &mut Lexer<'s>) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        let start = lexer.cur_pos()?;
        if lexer.cur()? != C_HYPHEN_MINUS || lexer.peek()? != C_HYPHEN_MINUS {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected starts with '--' during parsing of 'var()'",
                range: Range::new(start, lexer.peek2_pos()?),
            });
            return Some(());
        }
        lexer.consume_ident_sequence()?;
        let name_start = start + 2;
        let end = lexer.cur_pos()?;
        self.handle_dependency
            .handle_dependency(Dependency::LocalVar {
                name: lexer.slice(name_start, end)?,
                range: Range::new(start, end),
            });
        Some(())
    }

    fn lex_local_var_decl(
        &mut self,
        lexer: &mut Lexer<'s>,
        name: &'s str,
        start: Pos,
        end: Pos,
    ) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        if lexer.cur()? != C_COLON {
            return Some(());
        }
        lexer.consume();
        self.handle_dependency
            .handle_dependency(Dependency::LocalVarDecl {
                name,
                range: Range::new(start, end),
            });
        Some(())
    }

    fn lex_local_property_decl(&mut self, lexer: &mut Lexer<'s>) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        let start = lexer.cur_pos()?;
        if lexer.cur()? != C_HYPHEN_MINUS || lexer.peek()? != C_HYPHEN_MINUS {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected starts with '--' during parsing of '@property'",
                range: Range::new(start, lexer.peek2_pos()?),
            });
            return Some(());
        }
        lexer.consume_ident_sequence()?;
        let name_start = start + 2;
        let end = lexer.cur_pos()?;
        self.handle_dependency
            .handle_dependency(Dependency::LocalPropertyDecl {
                name: lexer.slice(name_start, end)?,
                range: Range::new(start, end),
            });
        lexer.consume_white_space_and_comments()?;
        if lexer.cur()? != C_LEFT_CURLY {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected '{' during parsing of '@property'",
                range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
            });
            return Some(());
        }
        Some(())
    }

    fn lex_local_keyframes_decl(&mut self, lexer: &mut Lexer<'s>) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        let mut is_function = false;
        if lexer.cur()? == C_COLON {
            let start = lexer.cur_pos()?;
            lexer.consume_potential_pseudo(self)?;
            let end = lexer.cur_pos()?;
            let pseudo = lexer.slice(start, end)?.to_ascii_lowercase();
            let mode_data = self.mode_data.as_ref().unwrap();
            if mode_data.is_pure_mode() && pseudo == ":global(" || pseudo == ":global" {
                self.handle_warning.handle_warning(Warning::NotPure {
                    range: Range::new(start, end),
                    message: "'@keyframes :global' is not allowed in pure mode",
                });
            }
            is_function = pseudo == ":local(" || pseudo == ":global(";
            if !is_function && pseudo != ":local" && pseudo != ":global" {
                self.handle_warning.handle_warning(Warning::Unexpected {
                    message: "Expected ':local', ':local()', ':global', or ':global()' during parsing of '@keyframes' name",
                    range: Range::new(start, end),
                });
                return Some(());
            }
            lexer.consume_white_space_and_comments()?;
        }
        let start = lexer.cur_pos()?;
        if !start_ident_sequence(lexer.cur()?, lexer.peek()?, lexer.peek2()?) {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected ident during parsing of '@keyframes' name",
                range: Range::new(start, lexer.peek2_pos()?),
            });
            return Some(());
        }
        lexer.consume_ident_sequence()?;
        let end = lexer.cur_pos()?;
        let mode_data = self.mode_data.as_ref().unwrap();
        if mode_data.is_current_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframesDecl {
                    name: lexer.slice(start, end)?,
                    range: Range::new(start, end),
                });
        }
        lexer.consume_white_space_and_comments()?;
        if is_function {
            if lexer.cur()? != C_RIGHT_PARENTHESIS {
                self.handle_warning.handle_warning(Warning::Unexpected {
                    message: "Expected ')' during parsing of '@keyframes :local(' or '@keyframes :global('",
                    range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
                });
                return Some(());
            }
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
                });
            self.balanced.pop_without_moda_data();
            lexer.consume();
            lexer.consume_white_space_and_comments()?;
        }
        if lexer.cur()? != C_LEFT_CURLY {
            self.handle_warning.handle_warning(Warning::Unexpected {
                message: "Expected '{' during parsing of '@keyframes'",
                range: Range::new(lexer.cur_pos()?, lexer.peek_pos()?),
            });
            return Some(());
        }
        Some(())
    }

    fn handle_local_keyframes_dependency(&mut self, lexer: &Lexer<'s>) -> Option<()> {
        if let Some(animation) = &mut self.in_animation_property {
            if let Some(range) = animation.take_keyframes(self.balanced.len()) {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalKeyframes {
                        name: lexer.slice(range.start, range.end)?,
                        range,
                    });
            }
            animation.reset_keywords();
        }
        Some(())
    }

    fn lex_composes(&mut self, lexer: &mut Lexer<'s>, start: Pos) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        if lexer.cur()? != C_COLON {
            return Some(());
        }
        lexer.consume();
        let mut end;
        let mut has_from = false;
        loop {
            lexer.consume_white_space_and_comments()?;
            let names_start = lexer.cur_pos()?;
            let mut names_end = names_start;
            loop {
                let name_start = lexer.cur_pos()?;
                let c = lexer.cur()?;
                if c == C_COMMA || c == C_SEMICOLON || c == C_RIGHT_CURLY {
                    break;
                }
                if !start_ident_sequence(c, lexer.peek()?, lexer.peek2()?) {
                    self.handle_warning.handle_warning(Warning::Unexpected {
                        message: "Expected ident during parsing of 'composes'",
                        range: Range::new(name_start, lexer.peek2_pos()?),
                    });
                    return Some(());
                }
                lexer.consume_ident_sequence()?;
                let name_end = lexer.cur_pos()?;
                if lexer.slice(name_start, name_end)? == "from" {
                    has_from = true;
                    break;
                }
                names_end = name_end;
                lexer.consume_white_space_and_comments()?;
            }
            lexer.consume_white_space_and_comments()?;
            let c = lexer.cur()?;
            if !has_from {
                self.handle_dependency
                    .handle_dependency(Dependency::Composes {
                        names: lexer.slice(names_start, names_end)?,
                        from: None,
                    });
                if c == C_COMMA {
                    lexer.consume();
                    continue;
                }
                end = names_end;
                break;
            }
            let path_start = lexer.cur_pos()?;
            if c == '\'' || c == '"' {
                lexer.consume();
                lexer.consume_string(self, c)?;
            } else if start_ident_sequence(c, lexer.peek()?, lexer.peek2()?) {
                lexer.consume_ident_sequence()?;
            } else {
                self.handle_warning.handle_warning(Warning::Unexpected {
                    message: "Expected string or ident during parsing of 'composes'",
                    range: Range::new(path_start, lexer.peek_pos()?),
                });
                return Some(());
            }
            let path_end = lexer.cur_pos()?;
            end = path_end;
            self.handle_dependency
                .handle_dependency(Dependency::Composes {
                    names: lexer.slice(names_start, names_end)?,
                    from: Some(lexer.slice(path_start, path_end)?),
                });
            lexer.consume_white_space_and_comments()?;
            if lexer.cur()? != C_COMMA {
                break;
            }
            lexer.consume();
        }
        if lexer.cur()? == C_SEMICOLON {
            lexer.consume();
            end = lexer.cur_pos()?;
        }
        self.handle_dependency
            .handle_dependency(Dependency::Replace {
                content: "",
                range: Range::new(start, end),
            });
        Some(())
    }
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> Visitor<'s> for LexDependencies<'s, D, W> {
    fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
        Some(self.is_next_rule_prelude)
    }

    fn url(
        &mut self,
        lexer: &mut Lexer<'s>,
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
                    self.handle_warning.handle_warning(Warning::DuplicateUrl {
                        range: Range::new(import_data.start, end),
                        when: lexer.slice(import_data.start, end)?,
                    });
                    return Some(());
                }
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

    fn string(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
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
                    self.handle_warning.handle_warning(Warning::DuplicateUrl {
                        range: Range::new(import_data.start, end),
                        when: lexer.slice(import_data.start, end)?,
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

    fn at_keyword(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if name == "@namespace" {
            self.scope = Scope::AtNamespaceInvalid;
            self.handle_warning
                .handle_warning(Warning::NamespaceNotSupportedInBundledCss {
                    range: Range::new(start, end),
                });
        } else if name == "@import" {
            if !self.allow_import_at_rule {
                self.scope = Scope::AtImportInvalid;
                self.handle_warning
                    .handle_warning(Warning::NotPrecededAtImport {
                        range: Range::new(start, end),
                    });
                return Some(());
            }
            self.scope = Scope::InAtImport(ImportData::new(start));
        } else if self.mode_data.is_some() {
            if name == "@keyframes" || with_at_vendor_prefixed_eq(&name, "keyframes") {
                self.lex_local_keyframes_decl(lexer)?;
            } else if name == "@property" {
                self.lex_local_property_decl(lexer)?;
            } else {
                self.is_next_rule_prelude = name == "@scope";
            }

            let mode_data = self.mode_data.as_mut().unwrap();
            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn semicolon(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        match self.scope {
            Scope::InAtImport(ref import_data) => {
                let Some(url) = import_data.url else {
                    self.handle_warning.handle_warning(Warning::ExpectedUrl {
                        range: Range::new(import_data.start, end),
                        when: lexer.slice(import_data.start, end)?,
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                let Some(url_range) = &import_data.url_range else {
                    self.handle_warning.handle_warning(Warning::Unexpected {
                        message: "Unexpected ';' during parsing of '@import url()'",
                        range: Range::new(start, end),
                    });
                    self.scope = Scope::TopLevel;
                    return Some(());
                };
                let layer = match &import_data.layer {
                    ImportDataLayer::None => None,
                    ImportDataLayer::EndLayer { value, range } => {
                        if url_range.start > range.start {
                            self.handle_warning
                                .handle_warning(Warning::ExpectedUrlBefore {
                                    range: url_range.clone(),
                                    when: lexer.slice(range.start, url_range.end)?,
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
                        self.handle_warning.handle_warning(Warning::Unexpected {
                            message: "Unexpected ';' during parsing of 'supports()'",
                            range: Range::new(start, end),
                        });
                        None
                    }
                    ImportDataSupports::EndSupports { value, range } => {
                        if url_range.start > range.start {
                            self.handle_warning
                                .handle_warning(Warning::ExpectedUrlBefore {
                                    range: url_range.clone(),
                                    when: lexer.slice(range.start, url_range.end)?,
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
                            self.handle_warning
                                .handle_warning(Warning::ExpectedLayerBefore {
                                    range: layer_range.clone(),
                                    when: lexer.slice(supports_range.start, layer_range.end)?,
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
                        self.handle_local_keyframes_dependency(lexer)?;
                        self.exit_animation_property();
                    }

                    self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
                }
            }
            _ => {}
        }
        Some(())
    }

    fn function(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        self.balanced.push(
            BalancedItem::new(&name, start, end),
            self.mode_data.as_mut(),
        );

        if let Scope::InAtImport(ref mut import_data) = self.scope {
            if name == "supports(" {
                import_data.supports = ImportDataSupports::InSupports;
            }
        }

        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_current_local_mode() && name == "var(" {
            self.lex_local_var(lexer)?;
        }
        Some(())
    }

    fn left_parenthesis(&mut self, _: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.balanced
            .push(BalancedItem::new_other(start, end), self.mode_data.as_mut());
        Some(())
    }

    fn right_parenthesis(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(last) = self.balanced.pop(self.mode_data.as_mut()) else {
            return Some(());
        };
        if let Some(mode_data) = &mut self.mode_data {
            let mut is_function = matches!(
                last.kind,
                BalancedItemKind::LocalFn | BalancedItemKind::GlobalFn
            );
            let is_class = matches!(
                last.kind,
                BalancedItemKind::LocalClass | BalancedItemKind::GlobalClass
            );
            if is_class {
                self.balanced.pop_mode_pseudo_class(mode_data);
                let popped = self.balanced.pop_without_moda_data().unwrap();
                debug_assert!(!matches!(
                    popped.kind,
                    BalancedItemKind::GlobalClass | BalancedItemKind::LocalClass
                ));
                is_function = matches!(
                    popped.kind,
                    BalancedItemKind::LocalFn | BalancedItemKind::GlobalFn
                );
            }
            if is_function {
                let distance = self.back_white_space_and_comments_distance(lexer, start)?;
                self.handle_dependency
                    .handle_dependency(Dependency::Replace {
                        content: "",
                        range: Range::new(start - distance, end),
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

    fn ident(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        match self.scope {
            Scope::InBlock => {
                let Some(mode_data) = &mut self.mode_data else {
                    return Some(());
                };
                if mode_data.is_property_local_mode() {
                    if let Some(animation) = &mut self.in_animation_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            animation
                                .set_keyframes(lexer.slice(start, end)?, Range::new(start, end));
                        }
                        return Some(());
                    }
                    let ident = lexer.slice(start, end)?;
                    let ident = ident.to_ascii_lowercase();
                    if ident == "animation"
                        || ident == "animation-name"
                        || with_vendor_prefixed_eq(&ident, "animation")
                        || with_vendor_prefixed_eq(&ident, "animation-name")
                    {
                        self.enter_animation_property();
                        return Some(());
                    }
                }
                if mode_data.is_current_local_mode() {
                    let ident = lexer.slice(start, end)?;
                    if let Some(name) = ident.strip_prefix("--") {
                        return self.lex_local_var_decl(lexer, name, start, end);
                    }
                    let ident = ident.to_ascii_lowercase();
                    if ident == "composes" {
                        return self.lex_composes(lexer, start);
                    }
                }
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

    fn class(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        if mode_data.is_current_local_mode() {
            let name = lexer.slice(start, end)?;
            self.handle_dependency
                .handle_dependency(Dependency::LocalIdent {
                    name,
                    range: Range::new(start, end),
                });
            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn id(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        if mode_data.is_current_local_mode() {
            let name = lexer.slice(start, end)?;
            self.handle_dependency
                .handle_dependency(Dependency::LocalIdent {
                    name,
                    range: Range::new(start, end),
                });
            if mode_data.is_pure_mode() {
                mode_data.pure_global = None;
            }
        }
        Some(())
    }

    fn left_curly_bracket(&mut self, lexer: &mut Lexer, start: Pos, _: Pos) -> Option<()> {
        match self.scope {
            Scope::TopLevel => {
                self.allow_import_at_rule = false;
                self.scope = Scope::InBlock;
                self.block_nesting_level = 1;
            }
            Scope::InBlock => {
                self.block_nesting_level += 1;
            }
            _ => return Some(()),
        }
        if let Some(mode_data) = &mut self.mode_data {
            if mode_data.is_pure_mode() && mode_data.pure_global.is_some() {
                let pure_global_start = mode_data.pure_global.unwrap();
                self.handle_warning.handle_warning(Warning::NotPure {
                    range: Range::new(pure_global_start, start),
                    message: "Selector is not pure (pure selectors must contain at least one local class or id)",
                });
            }

            if mode_data.resulting_global.is_some() && mode_data.is_current_local_mode() {
                let resulting_global_start = mode_data.resulting_global.unwrap();
                self.handle_warning
                    .handle_warning(Warning::InconsistentModeResult {
                        range: Range::new(resulting_global_start, start),
                    });
            }
            mode_data.resulting_global = None;

            self.balanced.update_property_mode(mode_data);
            self.balanced.pop_mode_pseudo_class(mode_data);
            self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
        }
        debug_assert!(
            self.balanced.is_empty(),
            "balanced should be empty when end of selector"
        );
        Some(())
    }

    fn right_curly_bracket(&mut self, lexer: &mut Lexer<'s>, _: Pos, end: Pos) -> Option<()> {
        if matches!(self.scope, Scope::InBlock) {
            if let Some(mode_data) = &mut self.mode_data {
                mode_data.pure_global = Some(end);

                if mode_data.is_property_local_mode() {
                    self.handle_local_keyframes_dependency(lexer)?;
                    self.exit_animation_property();
                }
            }

            self.block_nesting_level -= 1;
            if self.block_nesting_level == 0 {
                self.scope = Scope::TopLevel;
                if self.mode_data.is_some() {
                    self.is_next_rule_prelude = true;
                }
            } else if self.mode_data.is_some() {
                self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
            }
        }
        Some(())
    }

    fn pseudo_function(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if self.mode_data.is_some() && (name == ":global(" || name == ":local(") {
            if let Some(inside_start) = self.balanced.inside_mode_function() {
                self.handle_warning
                    .handle_warning(Warning::ExpectedNotInside {
                        range: Range::new(inside_start, end),
                        pseudo: lexer.slice(start, end)?,
                    });
            }

            lexer.consume_white_space_and_comments()?;
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, lexer.cur_pos()?),
                });
        }
        self.balanced.push(
            BalancedItem::new(&name, start, end),
            self.mode_data.as_mut(),
        );
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        if self.mode_data.is_none() {
            return Some(());
        };
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if name == ":global" || name == ":local" {
            if let Some(inside_start) = self.balanced.inside_mode_function() {
                self.handle_warning
                    .handle_warning(Warning::ExpectedNotInside {
                        range: Range::new(inside_start, end),
                        pseudo: lexer.slice(start, end)?,
                    });
            }

            let should_have_after_white_space = self.should_have_after_white_space(lexer, start);
            let has_after_white_space = self.has_after_white_space(lexer)?;
            let c = lexer.cur()?;
            if should_have_after_white_space
                && !(has_after_white_space
                    || c == C_RIGHT_PARENTHESIS
                    || c == C_LEFT_CURLY
                    || c == C_COMMA)
            {
                self.handle_warning
                    .handle_warning(Warning::MissingWhitespace {
                        range: Range::new(start, end),
                        surrounding: "trailing",
                    });
            }
            if !should_have_after_white_space && has_after_white_space {
                self.handle_warning
                    .handle_warning(Warning::MissingWhitespace {
                        range: Range::new(start, end),
                        surrounding: "leading",
                    });
            }

            self.balanced.push(
                BalancedItem::new(&name, start, end),
                self.mode_data.as_mut(),
            );
            let end2 = lexer.cur_pos()?;
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, end2),
                });
            return Some(());
        }
        if matches!(self.scope, Scope::TopLevel) && name == ":export" {
            self.lex_icss_export(lexer)?;
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, lexer.cur_pos()?),
                });
        }
        Some(())
    }

    fn comma(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };

        if mode_data.is_pure_mode() && mode_data.pure_global.is_some() {
            let pure_global_start = mode_data.pure_global.unwrap();
            self.handle_warning.handle_warning(Warning::NotPure {
                range: Range::new(pure_global_start, start),
                message: "Selector is not pure (pure selectors must contain at least one local class or id)",
            });
        }
        mode_data.pure_global = Some(end);

        if mode_data.resulting_global.is_some() && mode_data.is_current_local_mode() {
            let resulting_global_start = mode_data.resulting_global.unwrap();
            self.handle_warning
                .handle_warning(Warning::InconsistentModeResult {
                    range: Range::new(resulting_global_start, start),
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

        if matches!(self.scope, Scope::InBlock) && mode_data.is_property_local_mode() {
            self.handle_local_keyframes_dependency(lexer)?;
        }

        Some(())
    }
}
