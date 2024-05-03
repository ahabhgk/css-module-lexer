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

pub fn lex_css_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(handle_dependency, handle_warning, None);
    lexer.lex(&mut visitor);
}

pub fn collect_css_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(true)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

pub fn lex_css_modules_global_dependencies<'s>(
    input: &'s str,
    handle_dependency: impl FnMut(Dependency<'s>),
    handle_warning: impl FnMut(Warning),
) {
    let mut lexer = Lexer::from(input);
    let mut visitor = LexDependencies::new(
        handle_dependency,
        handle_warning,
        Some(CssModulesModeData::new(false)),
    );
    lexer.lex(&mut visitor);
}

pub fn collect_css_modules_global_dependencies(input: &str) -> (Vec<Dependency<'_>>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_css_modules_global_dependencies(input, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}
