//! Public dependency result types and their typed side-table storage.

use std::borrow::Cow;
use std::fmt::Display;
use std::marker::PhantomData;
use std::ops::Index;

use crate::lexer::Pos;

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

/// A half-open index range into one of [`DependencyContext`]'s flat payload
/// vectors. Keeping list payloads out of [`Dependency`] prevents rare, large
/// variants from determining the size of every dependency value. `T` binds the
/// range to its payload element type without adding runtime storage.
#[derive(Debug, Hash, PartialEq, Eq)]
pub struct DependencyListRange<T> {
    start: u32,
    end: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Copy for DependencyListRange<T> {}

impl<T> Clone for DependencyListRange<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> DependencyListRange<T> {
    pub(crate) fn from_bounds(start: usize, end: usize) -> Self {
        assert!(start <= end, "dependency list range is reversed");
        assert!(
            end <= u32::MAX as usize,
            "dependency list storage is too large"
        );
        Self {
            start: start as u32,
            end: end as u32,
            marker: PhantomData,
        }
    }

    pub(crate) fn as_usize_range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn start(self) -> u32 {
        self.start
    }

    pub fn end(self) -> u32 {
        self.end
    }

    pub fn len(self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Mode {
    Local,
    Global,
    Pure,
    Css,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ValueAtRuleImportItem<'s> {
    local_name: &'s str,
    import_name: &'s str,
}

impl<'s> ValueAtRuleImportItem<'s> {
    pub(crate) fn new(local_name: &'s str, import_name: &'s str) -> Self {
        Self {
            local_name,
            import_name,
        }
    }

    pub fn local_name(&self) -> &'s str {
        self.local_name
    }

    pub fn import_name(&self) -> &'s str {
        self.import_name
    }
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
        local_classes: DependencyListRange<Cow<'s, str>>,
        names: DependencyListRange<Cow<'s, str>>,
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

/// Owns dependencies and the flat list payloads referenced by them.
///
/// `Dependency` values are intentionally cheap to move. Variable-length data
/// is appended to a context vector and referenced through
/// [`DependencyListRange`]. Consumers should traverse and resolve those ranges
/// through this type rather than retaining raw list indices independently.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct DependencyContext<'s> {
    dependencies: Vec<Dependency<'s>>,
    composes_local_classes: Vec<Cow<'s, str>>,
    composes_names: Vec<Cow<'s, str>>,
    value_at_rule_import_items: Vec<ValueAtRuleImportItem<'s>>,
}

impl<'s> DependencyContext<'s> {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_dependency_capacity(capacity: usize) -> Self {
        Self {
            dependencies: Vec::with_capacity(capacity),
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.dependencies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Dependency<'s>> {
        self.dependencies.get(index)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Dependency<'s>> {
        self.dependencies.iter()
    }

    pub fn dependencies(&self) -> &[Dependency<'s>] {
        &self.dependencies
    }

    pub fn composes_local_classes(
        &self,
        range: DependencyListRange<Cow<'s, str>>,
    ) -> &[Cow<'s, str>] {
        &self.composes_local_classes[range.as_usize_range()]
    }

    pub fn composes_names(&self, range: DependencyListRange<Cow<'s, str>>) -> &[Cow<'s, str>] {
        &self.composes_names[range.as_usize_range()]
    }

    pub fn value_at_rule_import_items(&self) -> &[ValueAtRuleImportItem<'s>] {
        &self.value_at_rule_import_items
    }

    pub(crate) fn push_dependency(&mut self, dependency: Dependency<'s>) {
        self.dependencies.push(dependency);
    }

    pub(crate) fn value_at_rule_import_items_len(&self) -> usize {
        self.value_at_rule_import_items.len()
    }

    pub(crate) fn push_value_at_rule_import_item(&mut self, item: ValueAtRuleImportItem<'s>) {
        self.value_at_rule_import_items.push(item);
    }

    pub(crate) fn value_at_rule_import_item(&self, index: usize) -> ValueAtRuleImportItem<'s> {
        self.value_at_rule_import_items[index]
    }

    pub(crate) fn push_composes(
        &mut self,
        local_classes: impl IntoIterator<Item = Cow<'s, str>>,
        names: impl IntoIterator<Item = Cow<'s, str>>,
        from: Option<Cow<'s, str>>,
        range: Range,
    ) {
        let local_classes_start = self.composes_local_classes.len();
        self.composes_local_classes.extend(local_classes);
        let local_classes = DependencyListRange::<Cow<'s, str>>::from_bounds(
            local_classes_start,
            self.composes_local_classes.len(),
        );

        let names_start = self.composes_names.len();
        self.composes_names.extend(names);
        let names = DependencyListRange::<Cow<'s, str>>::from_bounds(
            names_start,
            self.composes_names.len(),
        );

        self.dependencies.push(Dependency::Composes {
            local_classes,
            names,
            from,
            range,
        });
    }
}

impl<'s> Index<usize> for DependencyContext<'s> {
    type Output = Dependency<'s>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.dependencies[index]
    }
}

impl<'context, 's> IntoIterator for &'context DependencyContext<'s> {
    type Item = &'context Dependency<'s>;
    type IntoIter = std::slice::Iter<'context, Dependency<'s>>;

    fn into_iter(self) -> Self::IntoIter {
        self.dependencies.iter()
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum UrlRangeKind {
    Function,
    String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Warning<'s> {
    pub(crate) range: Range,
    pub(crate) kind: WarningKind<'s>,
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
    NotPure { message: &'s str },
    UnexpectedComposition { message: &'s str },
}

impl Display for Warning<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            WarningKind::Unexpected { message, .. } => write!(f, "{message}"),
            WarningKind::DuplicateUrl { when, .. } => {
                write!(f, "Duplicate of 'url(...)' in '{when}'")
            }
            WarningKind::NamespaceNotSupportedInBundledCss { .. } => {
                write!(f, "'@namespace' is not supported in bundled CSS")
            }
            WarningKind::NotPrecededAtImport { .. } => {
                write!(f, "Any '@import' rules must precede all other rules")
            }
            WarningKind::ExpectedUrl { when, .. } => write!(f, "Expected URL in '{when}'"),
            WarningKind::ExpectedUrlBefore { when, .. } => {
                write!(f, "An URL in '{when}' should be before 'layer(...)' or 'supports(...)'")
            }
            WarningKind::ExpectedLayerBefore { when, .. } => {
                write!(f, "The 'layer(...)' in '{when}' should be before 'supports(...)'")
            }
            WarningKind::InconsistentModeResult { .. } => write!(
                f,
                "Inconsistent rule global/local (multiple selectors must result in the same mode for the rule)"
            ),
            WarningKind::ExpectedNotInside { pseudo, .. } => write!(
                f,
                "A '{pseudo}' is not allowed inside of a ':local()' or ':global()'"
            ),
            WarningKind::NotPure { message, .. } => {
                write!(f, "Pure globals is not allowed in pure mode, {message}")
            }
            WarningKind::UnexpectedComposition { message, .. } => {
                write!(f, "Composition is {message}")
            }
        }
    }
}
