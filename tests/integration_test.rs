use css_module_lexer::collect_dependencies;
use css_module_lexer::Dependency;
use css_module_lexer::Mode;
use css_module_lexer::WarningKind;

const RSPACK_CSS_MODULES: &str = include_str!("../fixtures/rspack-css-modules.css");

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

#[test]
fn rspack_css_modules() {
    assert!(RSPACK_CSS_MODULES.contains(":import("));
    assert!(RSPACK_CSS_MODULES.contains(":export"));

    let (local_dependencies, local_warnings) =
        collect_dependencies(RSPACK_CSS_MODULES, Mode::Local);
    assert!(!local_dependencies.is_empty());
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalClass { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalId { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalKeyframesDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalVar { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalPropertyDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalCounterStyleDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalFontPaletteDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalContainerDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalFunctionDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalGridDecl { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::Composes { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::ICSSImportFrom { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::ICSSImportValue { .. })));
    assert!(local_dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::ICSSExportValue { .. })));
    assert!(local_warnings
        .iter()
        .any(|warning| matches!(warning.kind(), WarningKind::MissingWhitespace { .. })));

    let (global_dependencies, global_warnings) =
        collect_dependencies(RSPACK_CSS_MODULES, Mode::Global);
    assert!(!global_dependencies.is_empty());
    assert!(!global_warnings.is_empty());

    let (pure_dependencies, pure_warnings) = collect_dependencies(RSPACK_CSS_MODULES, Mode::Pure);
    assert!(!pure_dependencies.is_empty());
    assert!(pure_warnings
        .iter()
        .any(|warning| matches!(warning.kind(), WarningKind::NotPure { .. })));
}
