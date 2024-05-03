use css_module_lexer::collect_css_modules_dependencies;

#[test]
fn bootstrap() {
    let input = include_str!("../fixtures/bootstrap.css");
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert!(!dependencies.is_empty());
}

#[test]
fn bootstrap_min() {
    let input = include_str!("../fixtures/bootstrap.min.css");
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert!(!dependencies.is_empty());
}
