use css_module_lexer::collect_dependencies;
use css_module_lexer::Mode;

#[test]
fn bootstrap() {
    let input = include_str!("../fixtures/bootstrap.css");
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());
    assert!(!dependencies.is_empty());
}

#[test]
fn bootstrap_min() {
    let input = include_str!("../fixtures/bootstrap.min.css");
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());
    assert!(!dependencies.is_empty());
}
