mod dependencies;
mod escape;
mod lexer;

pub use dependencies::Dependency;
pub use dependencies::LexDependencies;
pub use dependencies::Mode;
pub use dependencies::ModeData;
pub use dependencies::Range;
pub use dependencies::UrlRangeKind;
pub use dependencies::Warning;
pub use dependencies::WarningKind;
pub use escape::unescape;
pub use lexer::{Lexer, Pos, Token, TokenFlags, TokenKind, TokenWithTrivia, Trivia};

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
    visitor.lex_streaming(&mut lexer);
}

pub fn collect_dependencies(input: &str, mode: Mode) -> (Vec<Dependency<'_>>, Vec<Warning<'_>>) {
    let capacity = estimate_result_capacity(input.len(), mode);
    let mut dependencies = Vec::with_capacity(capacity.dependencies);
    let mut warnings = Vec::with_capacity(capacity.warnings);
    lex_dependencies(input, mode, |v| dependencies.push(v), |v| warnings.push(v));
    (dependencies, warnings)
}

struct CapacityEstimate {
    dependencies: usize,
    warnings: usize,
}

fn estimate_result_capacity(input_len: usize, mode: Mode) -> CapacityEstimate {
    let dependency_rate = match mode {
        Mode::Local | Mode::Pure => (1, 32),
        Mode::Global => (1, 256),
        Mode::Css => (1, 1024),
    };
    let warning_rate = match mode {
        Mode::Pure => (1, 256),
        _ => (1, 512),
    };

    CapacityEstimate {
        dependencies: estimate_capacity(input_len, dependency_rate.0, dependency_rate.1, 4, 8192),
        warnings: estimate_capacity(input_len, warning_rate.0, warning_rate.1, 2, 512),
    }
}

#[inline]
fn estimate_capacity(
    input_len: usize,
    numerator: usize,
    denominator: usize,
    minimum: usize,
    maximum: usize,
) -> usize {
    input_len
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(0)
        .clamp(minimum, maximum)
}
