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

    let (local_dependencies, _local_warnings) =
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
    // Dependency extraction intentionally does not preserve legacy
    // MissingWhitespace diagnostics for mode pseudos.

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

#[test]
fn declaration_values_do_not_start_property_parsers() {
    for input in [
        ".a { color: animation spin; }",
        ".a { color: list-style custom; }",
        ".a { color: container card; }",
        ".a { color: grid area; }",
        ".a { color: composes foo; }",
        ".a { color: --foo; }",
    ] {
        let (dependencies, _) = collect_dependencies(input, Mode::Local);
        assert!(
            !dependencies.iter().any(|dependency| matches!(
                dependency,
                Dependency::LocalKeyframes { .. }
                    | Dependency::LocalCounterStyle { .. }
                    | Dependency::LocalContainerDecl { .. }
                    | Dependency::LocalGrid { .. }
                    | Dependency::Composes { .. }
                    | Dependency::LocalVarDecl { .. }
            )),
            "declaration value produced a property dependency for {input:?}: {dependencies:#?}"
        );
    }
}

#[test]
fn generic_value_fast_forward_preserves_dependency_candidates() {
    let input = r#"
        @value imported: red;
        .a {
            color: calc(imported + 1px);
            outline-color: imp\6f rted;
            background: image-set("a.png" 1x, url("b.png") 2x);
            width: var(--token);
            height: --scale(2);
            border-color: #fff;
            content: "url(not-a-dependency.png)";
        }
    "#;
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());

    assert_eq!(
        dependencies
            .iter()
            .filter(|dependency| matches!(
                dependency,
                Dependency::ICSSSymbol { name, .. } if name.as_ref() == "imported"
            ))
            .count(),
        2
    );
    assert_eq!(
        dependencies
            .iter()
            .filter(|dependency| matches!(dependency, Dependency::Url { .. }))
            .count(),
        2
    );
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::LocalVar { name, .. } if name.as_ref() == "token"
    )));
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::LocalFunction { name, .. } if name.as_ref() == "scale"
    )));
    assert!(!dependencies
        .iter()
        .any(|dependency| matches!(dependency, Dependency::LocalId { .. })));
}
