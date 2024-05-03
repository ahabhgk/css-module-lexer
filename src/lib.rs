mod dependencies;
mod lexer;

pub use dependencies::CssModulesModeData;
pub use dependencies::Dependency;
pub use dependencies::LexDependencies;
pub use dependencies::Range;
pub use dependencies::UrlRangeKind;
pub use dependencies::Warning;
pub use lexer::Lexer;
pub use lexer::Pos;
pub use lexer::Visitor;

pub trait HandleDependency<'s> {
    fn handle_dependency(&mut self, dependency: Dependency<'s>);
}

pub trait HandleWarning<'s> {
    fn handle_warning(&mut self, warning: Warning<'s>);
}

impl<'s, F: FnMut(Dependency<'s>)> HandleDependency<'s> for F {
    fn handle_dependency(&mut self, dependency: Dependency<'s>) {
        self(dependency);
    }
}

impl<'s, F: FnMut(Warning<'s>)> HandleWarning<'s> for F {
    fn handle_warning(&mut self, warning: Warning<'s>) {
        self(warning);
    }
}

pub fn lex_css_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl HandleDependency<'s>,
    handle_warning: impl HandleWarning<'s>,
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(handle_dependency, handle_warning, None);
    lexer.lex(&mut visitor);
}

pub fn collect_css_dependencies(input: &str) -> (Vec<Dependency>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl HandleDependency<'s>,
    handle_warning: impl HandleWarning<'s>,
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(true)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning<'_>>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_global_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl HandleDependency<'s>,
    handle_warning: impl HandleWarning<'s>,
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(false)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_global_dependencies(
    input: &str,
) -> (Vec<Dependency<'_>>, Vec<Warning<'_>>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_global_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}
