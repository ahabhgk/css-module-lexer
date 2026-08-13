use css_module_lexer::Dependency;
use css_module_lexer::Mode;
use css_module_lexer::WarningKind;
use css_module_lexer::collect_dependencies;

const RSPACK_CSS_MODULES: &str = include_str!("../fixtures/rspack-css-modules.css");

fn dashed_ident_slices<'a>(input: &'a str, ranges: &[css_module_lexer::Range]) -> Vec<&'a str> {
    ranges
        .iter()
        .map(|range| &input[range.start as usize..range.end as usize])
        .collect()
}

#[test]
fn dashed_ident_collection_is_enabled_for_modules_and_disabled_for_css_mode() {
    let input = ".local { --token: var(--token); }";
    let (module_context, _) = collect_dependencies(input, Mode::Local);
    assert_eq!(module_context.dashed_ident_occurrences().len(), 2);

    let (css_context, _) = collect_dependencies(input, Mode::Css);
    assert!(css_context.dashed_ident_occurrences().is_empty());
}

#[test]
fn dashed_ident_collection_uses_the_single_token_pass() {
    let input = r#"
        .local {
            before: --used-before-declaration;
            --token: var(--token);
            global-reference: var(--token from global);
            style: style(--style-token);
            call: --custom-function(red);
            string: "--inside-string";
            image: url(--inside-url);
            /* --inside-comment */
        }
        :global .global { value: --global-token; }
        @property --property-token { syntax: "*"; inherits: false; }
        @value --value-token: red;
        :export { --export-token: red; }
    "#;
    let (context, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());
    assert_eq!(
        dashed_ident_slices(input, context.dashed_ident_occurrences()),
        [
            "--used-before-declaration",
            "--token",
            "--token",
            "--style-token",
            "--custom-function",
            "--global-token",
            "--property-token",
            "--value-token",
            "--export-token",
        ]
    );
}

#[test]
fn dashed_ident_collection_preserves_escaped_ranges() {
    let input = r#".local { \2d\2d token: var(\2d\2d token); }"#;
    let (context, warnings) = collect_dependencies(input, Mode::Local);
    assert!(!warnings.is_empty());
    assert_eq!(
        dashed_ident_slices(input, context.dashed_ident_occurrences()),
        [r#"\2d\2d token"#, r#"\2d\2d token"#]
    );
}

#[test]
fn icss_comment_like_string_is_not_stripped_by_a_consumer() {
    let input = r#":export { token: "/* not a comment */"; }"#;
    let (context, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());
    assert!(context.iter().any(|dependency| matches!(
        dependency,
        Dependency::ICSSExportValue { prop: "token", value }
            if *value == r#""/* not a comment */""#
    )));
}

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
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalClass { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalId { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalKeyframesDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalVar { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalPropertyDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalCounterStyleDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalFontPaletteDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalContainerDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalFunctionDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalGridDecl { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::Composes { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::ICSSImportFrom { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::ICSSImportValue { .. }))
    );
    assert!(
        local_dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::ICSSExportValue { .. }))
    );
    // Dependency extraction intentionally does not preserve legacy
    // MissingWhitespace diagnostics for mode pseudos.

    let (global_dependencies, global_warnings) =
        collect_dependencies(RSPACK_CSS_MODULES, Mode::Global);
    assert!(!global_dependencies.is_empty());
    assert!(!global_warnings.is_empty());

    let (pure_dependencies, pure_warnings) = collect_dependencies(RSPACK_CSS_MODULES, Mode::Pure);
    assert!(!pure_dependencies.is_empty());
    assert!(
        pure_warnings
            .iter()
            .any(|warning| matches!(warning.kind(), WarningKind::NotPure { .. }))
    );
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
                Dependency::ICSSSymbol { name, .. } if *name == "imported"
            ))
            .count(),
        1
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
        Dependency::LocalVar { name, .. } if *name == "token"
    )));
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::LocalFunction { name, .. } if *name == "scale"
    )));
    assert!(
        !dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalId { .. }))
    );
}

#[test]
fn selector_fast_forward_preserves_candidates_and_opaque_ranges() {
    let input = r##"
        /* selector-fast-forward-padding-selector-fast-forward-padding-selector-fast-forward-padding-selector-fast-forward-padding-selector-fast-forward-padding-selector-fast-forward-padding */
        element:is(.one, #two):not([data-value=".fake:#fake:local"]) > :global(.global) {}
        .local[data-value="#fake.local:global"] {}
        :local/**/.commented {}
    "##;
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert_eq!(
        warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["Missing trailing whitespace"]
    );

    let selector_dependencies = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::LocalClass { name, .. } | Dependency::LocalId { name, .. } => Some(*name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selector_dependencies,
        [".one", "#two", ".local", ".commented"]
    );
}

#[test]
fn selector_fast_forward_skips_escaped_brackets_in_attributes() {
    let input = format!(
        "/*{}*/ .root[data=\\[] .real {{}}",
        "selector-fast-forward-padding".repeat(10)
    );
    assert!(input.len() >= 256);

    let (dependencies, warnings) = collect_dependencies(&input, Mode::Local);
    assert!(warnings.is_empty());
    let local_classes = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::LocalClass { name, .. } => Some(*name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(local_classes, [".root", ".real"]);
}

#[test]
fn selector_fast_forward_keeps_composes_validation() {
    for selector in ["element.local", "[data-kind].local", "*.local"] {
        let input = format!(
            "{} {selector} {{ composes: base; }}",
            "long-element-name".repeat(16)
        );
        let (dependencies, _) = collect_dependencies(&input, Mode::Local);
        assert!(
            !dependencies
                .iter()
                .any(|dependency| matches!(dependency, Dependency::Composes { .. })),
            "complex selector unexpectedly allowed composes: {selector}"
        );
    }
}

#[test]
fn at_rule_fast_forward_preserves_boundaries_and_dependency_candidates() {
    let input = r#"
        @value imported: 1px;
        @media screen and (min-width: 1px) {
            .media { background: url("media.png"); }
        }
        @supports selector(.prelude-only) {
            .supports { width: var(--size); }
        }
        .root {
            @unknown wrapper(foo; "literal;{") imported url("nested.png");
            color: red;
        }
    "#;
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());

    let local_classes = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::LocalClass { name, .. } => Some(*name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(local_classes, [".media", ".supports", ".root"]);

    let urls = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::Url { request, .. } => Some(*request),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(urls, ["media.png", "nested.png"]);
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::LocalVar { name, .. } if *name == "size"
    )));
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::ICSSSymbol { name, .. } if *name == "imported"
    )));
}

#[test]
fn custom_property_token_blocks_do_not_create_selector_dependencies() {
    let input = r##"
        .owner {
            --braces: { 1, "string", .not-a-selector, #f1f1f1, #not-an-id };
            --brackets: [ 1, "string", .not-a-selector, #f1f1f1, #not-an-id ];
            --parentheses: ( 1, "string", .not-a-selector, #f1f1f1, #not-an-id );
        }
    "##;
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());

    let local_classes = dependencies
        .iter()
        .filter_map(|dependency| match dependency {
            Dependency::LocalClass { name, .. } => Some(*name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(local_classes, [".owner"]);
    assert!(
        !dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::LocalId { .. }))
    );
}

#[test]
fn icss_symbols_are_looked_up_only_in_value_contexts() {
    let input = r#"
        @value imported: red;
        .imported {
            imported: imported;
            animation: imported;
            @media (min-width: imported) {
                .inside { color: red; }
            }
        }
    "#;
    let (dependencies, warnings) = collect_dependencies(input, Mode::Local);
    assert!(warnings.is_empty());

    assert_eq!(
        dependencies
            .iter()
            .filter(|dependency| matches!(
                dependency,
                Dependency::ICSSSymbol { name, .. } if *name == "imported"
            ))
            .count(),
        3
    );
    assert!(dependencies.iter().any(|dependency| matches!(
        dependency,
        Dependency::LocalClass { name, .. } if *name == ".imported"
    )));
}
