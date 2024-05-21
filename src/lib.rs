mod dependencies;
mod lexer;

pub use dependencies::Dependency;
pub use dependencies::LexDependencies;
pub use dependencies::Mode;
pub use dependencies::ModeData;
pub use dependencies::Range;
pub use dependencies::UrlRangeKind;
pub use dependencies::Warning;
pub use dependencies::WarningKind;
pub use lexer::Lexer;
pub use lexer::Pos;

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

pub fn lex_dependencies<'s>(
    input: &'s str,
    mode: Mode,
    handle_dependency: impl HandleDependency<'s>,
    handle_warning: impl HandleWarning<'s>,
) {
    let mut lexer = Lexer::new(input);
    let mut visitor = LexDependencies::new(handle_dependency, handle_warning, mode);
    lexer.lex(&mut visitor);
}

pub fn collect_dependencies(input: &str, mode: Mode) -> (Vec<Dependency>, Vec<Warning>) {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    lex_dependencies(input, mode, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}
