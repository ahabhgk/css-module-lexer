use std::fmt::Display;

use smallvec::SmallVec;

use crate::lexer::is_white_space;
use crate::lexer::start_ident_sequence;
use crate::lexer::C_ASTERISK;
use crate::lexer::C_COLON;
use crate::lexer::C_COMMA;
use crate::lexer::C_HYPHEN_MINUS;
use crate::lexer::C_LEFT_CURLY;
use crate::lexer::C_RIGHT_CURLY;
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

    pub fn push(&mut self, item: BalancedItem, mode_data: Option<&mut CssModulesModeData>) {
        if let Some(mode_data) = mode_data {
            if matches!(
                item.kind,
                BalancedItemKind::LocalFn | BalancedItemKind::LocalClass
            ) {
                mode_data.set_local();
            } else if matches!(
                item.kind,
                BalancedItemKind::GlobalFn | BalancedItemKind::GlobalClass
            ) {
                mode_data.set_global();
            }
        }
        self.0.push(item);
    }

    pub fn pop(&mut self, mode_data: Option<&mut CssModulesModeData>) -> Option<BalancedItem> {
        let item = self.0.pop()?;
        if let Some(mode_data) = mode_data {
            let mut iter = self.0.iter();
            loop {
                if let Some(last) = iter.next_back() {
                    if matches!(
                        last.kind,
                        BalancedItemKind::LocalFn | BalancedItemKind::LocalClass
                    ) {
                        mode_data.set_local();
                        break;
                    } else if matches!(
                        last.kind,
                        BalancedItemKind::GlobalFn | BalancedItemKind::GlobalClass
                    ) {
                        mode_data.set_global();
                        break;
                    }
                } else {
                    mode_data.set_default();
                    break;
                }
            }
        }
        Some(item)
    }

    pub fn pop_without_moda_data(&mut self) -> Option<BalancedItem> {
        self.0.pop()
    }

    pub fn pop_modules_mode_pseudo_class(&mut self, mode_data: &mut CssModulesModeData) {
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
        mode_data.set_default();
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

#[derive(Debug)]
enum CssModulesMode {
    Local,
    Global,
    None,
}

#[derive(Debug)]
pub struct CssModulesModeData {
    default: CssModulesMode,
    current: CssModulesMode,
}

impl CssModulesModeData {
    pub fn new(local: bool) -> Self {
        Self {
            default: if local {
                CssModulesMode::Local
            } else {
                CssModulesMode::Global
            },
            current: CssModulesMode::None,
        }
    }

    pub fn is_local_mode(&self) -> bool {
        match self.current {
            CssModulesMode::Local => true,
            CssModulesMode::Global => false,
            CssModulesMode::None => match self.default {
                CssModulesMode::Local => true,
                CssModulesMode::Global => false,
                CssModulesMode::None => false,
            },
        }
    }

    pub fn set_local(&mut self) {
        self.current = CssModulesMode::Local;
    }

    pub fn set_global(&mut self) {
        self.current = CssModulesMode::Global;
    }

    pub fn set_default(&mut self) {
        self.current = CssModulesMode::None;
    }
}

#[derive(Debug, Default)]
enum AnimationKeyframes {
    #[default]
    Start,
    LastIdent(Range),
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
}

impl Display for Warning<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Warning::Unexpected { message, .. } => write!(f, "{message}"),
            Warning::DuplicateUrl { when, .. } => {
                write!(f, "Duplicate of 'url(...)' in '{when}'")
            }
            Warning::NamespaceNotSupportedInBundledCss { .. } => {
                write!(f, "'@namespace' is not supported in bundled CSS")
            }
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
        }
    }
}

#[derive(Debug)]
pub struct LexDependencies<'s, D, W> {
    mode_data: Option<CssModulesModeData>,
    scope: Scope<'s>,
    block_nesting_level: u32,
    allow_import_at_rule: bool,
    balanced: BalancedStack,
    is_next_rule_prelude: bool,
    in_animation_property: Option<AnimationKeyframes>,
    handle_dependency: D,
    handle_warning: W,
}

impl<'s, D: HandleDependency<'s>, W: HandleWarning<'s>> LexDependencies<'s, D, W> {
    pub fn new(
        handle_dependency: D,
        handle_warning: W,
        mode_data: Option<CssModulesModeData>,
    ) -> Self {
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
        media_lexer.consume()?;
        media_lexer.consume_white_space_and_comments()?;
        Some(media)
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
            lexer.consume()?;
        }
        Some(())
    }

    fn consume_icss_export_value(&self, lexer: &mut Lexer<'s>) -> Option<()> {
        loop {
            let c = lexer.cur()?;
            if c == C_RIGHT_CURLY || c == C_SEMICOLON {
                break;
            }
            lexer.consume()?;
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
        lexer.consume()?;
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
            lexer.consume()?;
            lexer.consume_white_space_and_comments()?;
            let value_start = lexer.cur_pos()?;
            self.consume_icss_export_value(lexer)?;
            let value_end = lexer.cur_pos()?;
            if lexer.cur()? == C_SEMICOLON {
                lexer.consume()?;
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
        lexer.consume()?;
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
        lexer.consume()?;
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
        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        lexer.consume_white_space_and_comments()?;
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
        if mode_data.is_local_mode() {
            self.handle_dependency
                .handle_dependency(Dependency::LocalKeyframesDecl {
                    name: lexer.slice(start, end)?,
                    range: Range::new(start, end),
                });
        }
        lexer.consume_white_space_and_comments()?;
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
            if let AnimationKeyframes::LastIdent(range) = std::mem::take(animation) {
                self.handle_dependency
                    .handle_dependency(Dependency::LocalKeyframes {
                        name: lexer.slice(range.start, range.end)?,
                        range,
                    });
            }
        }
        Some(())
    }

    fn lex_composes(&mut self, lexer: &mut Lexer<'s>, start: Pos) -> Option<()> {
        lexer.consume_white_space_and_comments()?;
        if lexer.cur()? != C_COLON {
            return Some(());
        }
        lexer.consume()?;
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
                    lexer.consume()?;
                    continue;
                }
                end = names_end;
                break;
            }
            let path_start = lexer.cur_pos()?;
            if c == '\'' || c == '"' {
                lexer.consume()?;
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
            lexer.consume()?;
        }
        if lexer.cur()? == C_SEMICOLON {
            lexer.consume()?;
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
            if name == "@keyframes" {
                self.lex_local_keyframes_decl(lexer)?;
            } else if name == "@property" {
                self.lex_local_property_decl(lexer)?;
            } else {
                self.is_next_rule_prelude = name == "@scope";
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
                if let Some(mode_data) = &self.mode_data {
                    if mode_data.is_local_mode() {
                        self.handle_local_keyframes_dependency(lexer)?;
                        self.in_animation_property = None;
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
        if mode_data.is_local_mode() {
            // Don't rename animation name when we in functions
            if let Some(animation) = &mut self.in_animation_property {
                if !self.balanced.is_empty() {
                    *animation = AnimationKeyframes::Start;
                }
            }
            if name == "var(" {
                self.lex_local_var(lexer)?;
            }
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
        if self.mode_data.is_some() {
            let is_function = matches!(
                last.kind,
                BalancedItemKind::LocalFn | BalancedItemKind::GlobalFn
            );
            if is_function
                || matches!(
                    last.kind,
                    BalancedItemKind::LocalClass | BalancedItemKind::GlobalClass
                )
            {
                if is_function {
                    self.handle_dependency
                        .handle_dependency(Dependency::Replace {
                            content: "",
                            range: Range::new(start, end),
                        });
                } else {
                    let popped = self.balanced.pop_without_moda_data();
                    debug_assert!(matches!(popped.unwrap().kind, BalancedItemKind::Other));
                }
                return Some(());
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
                if mode_data.is_local_mode() {
                    if let Some(animation) = &mut self.in_animation_property {
                        // Not inside functions
                        if self.balanced.is_empty() {
                            *animation = AnimationKeyframes::LastIdent(Range::new(start, end));
                        }
                        return Some(());
                    }
                    let ident = lexer.slice(start, end)?;
                    if let Some(name) = ident.strip_prefix("--") {
                        return self.lex_local_var_decl(lexer, name, start, end);
                    }
                    let ident = ident.to_ascii_lowercase();
                    if ident == "animation" || ident == "animation-name" {
                        self.in_animation_property = Some(AnimationKeyframes::Start);
                        return Some(());
                    }
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
        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_local_mode() {
            let name = lexer.slice(start, end)?;
            self.handle_dependency
                .handle_dependency(Dependency::LocalIdent {
                    name,
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn id(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        let Some(mode_data) = &self.mode_data else {
            return Some(());
        };
        if mode_data.is_local_mode() {
            let name = lexer.slice(start, end)?;
            self.handle_dependency
                .handle_dependency(Dependency::LocalIdent {
                    name,
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn left_curly_bracket(&mut self, lexer: &mut Lexer, _: Pos, _: Pos) -> Option<()> {
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
            self.balanced.pop_modules_mode_pseudo_class(mode_data);
            self.is_next_rule_prelude = self.is_next_nested_syntax(lexer)?;
        }
        debug_assert!(
            self.balanced.is_empty(),
            "balanced should be empty when end of selector"
        );
        Some(())
    }

    fn right_curly_bracket(&mut self, lexer: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        if matches!(self.scope, Scope::InBlock) {
            if let Some(mode_data) = &self.mode_data {
                if mode_data.is_local_mode() {
                    self.handle_local_keyframes_dependency(lexer)?;
                    self.in_animation_property = None;
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

    fn pseudo_function(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        self.balanced.push(
            BalancedItem::new(&name, start, end),
            self.mode_data.as_mut(),
        );
        if self.mode_data.is_some() && (name == ":global(" || name == ":local(") {
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: "",
                    range: Range::new(start, end),
                });
        }
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut Lexer<'s>, start: Pos, end: Pos) -> Option<()> {
        if self.mode_data.is_none() {
            return Some(());
        };
        let name = lexer.slice(start, end)?.to_ascii_lowercase();
        if name == ":global" || name == ":local" {
            self.balanced.push(
                BalancedItem::new(&name, start, end),
                self.mode_data.as_mut(),
            );
            lexer.consume_white_space_and_comments()?;
            let end2 = lexer.cur_pos()?;
            let comments = lexer.slice(end, end2)?.trim_matches(is_white_space);
            self.handle_dependency
                .handle_dependency(Dependency::Replace {
                    content: comments,
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

    fn comma(&mut self, lexer: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        let Some(mode_data) = &mut self.mode_data else {
            return Some(());
        };
        if let Some(last) = self.balanced.last() {
            if matches!(
                last.kind,
                BalancedItemKind::LocalClass | BalancedItemKind::GlobalClass
            ) && self.balanced.len() == 1
            {
                self.balanced.pop_modules_mode_pseudo_class(mode_data);
            }
        }
        if matches!(self.scope, Scope::InBlock) && mode_data.is_local_mode() {
            self.handle_local_keyframes_dependency(lexer)?;
        }
        Some(())
    }
}
