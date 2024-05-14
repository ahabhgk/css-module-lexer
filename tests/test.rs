mod postcss_modules;

use css_module_lexer::collect_css_dependencies;
use css_module_lexer::collect_css_modules_dependencies;
use css_module_lexer::Dependency;
use css_module_lexer::Lexer;
use css_module_lexer::UrlRangeKind;
use css_module_lexer::Warning;
use indoc::indoc;

fn assert_warning(input: &str, warning: &Warning, range_content: &str) {
    match warning {
        Warning::Unexpected { range, .. }
        | Warning::DuplicateUrl { range, .. }
        | Warning::NamespaceNotSupportedInBundledCss { range }
        | Warning::NotPrecededAtImport { range }
        | Warning::ExpectedUrl { range, .. }
        | Warning::ExpectedUrlBefore { range, .. }
        | Warning::ExpectedLayerBefore { range, .. }
        | Warning::InconsistentModeResult { range }
        | Warning::ExpectedNotInside { range, .. }
        | Warning::MissingWhitespace { range, .. }
        | Warning::NotPure { range, .. } => {
            assert_eq!(Lexer::slice_range(input, range).unwrap(), range_content);
        }
    };
}

fn assert_url_dependency(
    input: &str,
    dependency: &Dependency,
    request: &str,
    kind: UrlRangeKind,
    range_content: &str,
) {
    let Dependency::Url {
        request: req,
        range,
        kind: k,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*req, request);
    assert_eq!(*k, kind);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), range_content);
}

fn assert_import_dependency(
    input: &str,
    dependency: &Dependency,
    request: &str,
    layer: Option<&str>,
    supports: Option<&str>,
    media: Option<&str>,
    range_content: &str,
) {
    let Dependency::Import {
        request: actual_request,
        range,
        layer: actual_layer,
        supports: actual_supports,
        media: actual_media,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_request, request);
    assert_eq!(*actual_layer, layer);
    assert_eq!(*actual_supports, supports);
    assert_eq!(*actual_media, media);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), range_content);
}

fn assert_local_ident_dependency(input: &str, dependency: &Dependency, name: &str, explicit: bool) {
    let Dependency::LocalIdent {
        name: actual_name,
        explicit: actual_explicit,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(*actual_explicit, explicit);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), name);
}

fn assert_local_var_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalVar {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(
        Lexer::slice_range(input, range).unwrap(),
        format!("--{}", name)
    );
}

fn assert_local_var_decl_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalVarDecl {
        range,
        name: actual_name,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(
        Lexer::slice_range(input, range).unwrap(),
        format!("--{}", name)
    );
}

fn assert_local_property_decl_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalPropertyDecl {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(
        Lexer::slice_range(input, range).unwrap(),
        format!("--{}", name)
    );
}

fn assert_local_keyframes_decl_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalKeyframesDecl {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), name);
}

fn assert_local_keyframes_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalKeyframes {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), name);
}

fn assert_composes_dependency(
    _input: &str,
    dependency: &Dependency,
    names: &str,
    from: Option<&str>,
) {
    let Dependency::Composes {
        names: actual_names,
        from: actual_from,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_names, names);
    assert_eq!(*actual_from, from);
}

fn assert_replace_dependency(
    input: &str,
    dependency: &Dependency,
    content: &str,
    range_content: &str,
) {
    let Dependency::Replace {
        content: actual_content,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_content, content);
    assert_eq!(Lexer::slice_range(input, range).unwrap(), range_content);
}

fn assert_icss_import_from_dependency(_input: &str, dependency: &Dependency, path: &str) {
    let Dependency::ICSSImportFrom { path: actual_path } = dependency else {
        return assert!(false);
    };
    assert_eq!(*actual_path, path);
}

fn assert_icss_import_value_dependency(
    _input: &str,
    dependency: &Dependency,
    prop: &str,
    value: &str,
) {
    let Dependency::ICSSImportValue {
        prop: actual_prop,
        value: actual_value,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_prop, prop);
    assert_eq!(*actual_value, value);
}

fn assert_icss_export_value_dependency(
    _input: &str,
    dependency: &Dependency,
    prop: &str,
    value: &str,
) {
    let Dependency::ICSSExportValue {
        prop: actual_prop,
        value: actual_value,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_prop, prop);
    assert_eq!(*actual_value, value);
}

#[test]
fn empty() {
    let (dependencies, warnings) = collect_css_dependencies("");
    assert!(warnings.is_empty());
    assert!(dependencies.is_empty());
}

#[test]
fn url() {
    let input = indoc! {r#"
        body {
            background: url(
                https://example\2f4a8f.com\
        /image.png
            )
        }
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_url_dependency(
        input,
        &dependencies[0],
        "https://example\\2f4a8f.com\\\n/image.png",
        UrlRangeKind::Function,
        "url(\n        https://example\\2f4a8f.com\\\n/image.png\n    )",
    );
}

#[test]
fn duplicate_url() {
    let input = indoc! {r#"
        @import url(./a.css) url(./a.css);
        @import url(./a.css) url("./a.css");
        @import url("./a.css") url(./a.css);
        @import url("./a.css") url("./a.css");
    "#};
    let (_, warnings) = collect_css_dependencies(input);
    assert_warning(input, &warnings[0], "@import url(./a.css) url(./a.css)");
    assert_warning(input, &warnings[1], "@import url(./a.css) url(\"./a.css\"");
    assert_warning(input, &warnings[2], "@import url(\"./a.css\") url(./a.css)");
    assert_warning(
        input,
        &warnings[3],
        "@import url(\"./a.css\") url(\"./a.css\"",
    );
}

#[test]
fn not_preceded_at_import() {
    let input = indoc! {r#"
        body {}
        @import url(./a.css);
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(dependencies.is_empty());
    assert_warning(input, &warnings[0], "@import");
}

#[test]
fn url_string() {
    let input = indoc! {r#"
        body {
            a: url("https://example\2f4a8f.com\
            /image.png");
            b: image-set(
                "image1.png" 1x,
                "image2.png" 2x
            );
            c: image-set(
                url(image1.avif) type("image/avif"),
                url("image2.jpg") type("image/jpeg")
            );
        }
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_url_dependency(
        input,
        &dependencies[0],
        "https://example\\2f4a8f.com\\\n    /image.png",
        UrlRangeKind::String,
        "\"https://example\\2f4a8f.com\\\n    /image.png\"",
    );
    assert_url_dependency(
        input,
        &dependencies[1],
        "image1.png",
        UrlRangeKind::Function,
        "\"image1.png\"",
    );
    assert_url_dependency(
        input,
        &dependencies[2],
        "image2.png",
        UrlRangeKind::Function,
        "\"image2.png\"",
    );
    assert_url_dependency(
        input,
        &dependencies[3],
        "image1.avif",
        UrlRangeKind::Function,
        "url(image1.avif)",
    );
    assert_url_dependency(
        input,
        &dependencies[4],
        "image2.jpg",
        UrlRangeKind::String,
        "\"image2.jpg\"",
    );
}

#[test]
fn empty_url() {
    let input = indoc! {r#"
        @import url();
        @import url("");
        body {
            a: url();
            b: url("");
            c: image-set(); // not an dependency
            d: image-set("");
            e: image-set(url());
            f: image-set(url(""));
        }
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_import_dependency(
        input,
        &dependencies[0],
        "",
        None,
        None,
        None,
        "@import url();",
    );
    assert_import_dependency(
        input,
        &dependencies[1],
        "",
        None,
        None,
        None,
        "@import url(\"\");",
    );
    assert_url_dependency(input, &dependencies[2], "", UrlRangeKind::Function, "url()");
    assert_url_dependency(input, &dependencies[3], "", UrlRangeKind::String, "\"\"");
    assert_url_dependency(input, &dependencies[4], "", UrlRangeKind::Function, "\"\"");
    assert_url_dependency(input, &dependencies[5], "", UrlRangeKind::Function, "url()");
    assert_url_dependency(input, &dependencies[6], "", UrlRangeKind::String, "\"\"");
}

#[test]
fn expect_url() {
    let input = indoc! {r#"
        @import ;
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(dependencies.is_empty());
    assert_warning(&input, &warnings[0], "@import ;");
}

#[test]
fn import() {
    let input = indoc! {r#"
        @import 'https://example\2f4a8f.com\
        /style.css';
        @import url(https://example\2f4a8f.com\
        /style.css);
        @import url('https://example\2f4a8f.com\
        /style.css') /* */;
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_import_dependency(
        input,
        &dependencies[0],
        "https://example\\2f4a8f.com\\\n/style.css",
        None,
        None,
        None,
        "@import 'https://example\\2f4a8f.com\\\n/style.css';",
    );
    assert_import_dependency(
        input,
        &dependencies[1],
        "https://example\\2f4a8f.com\\\n/style.css",
        None,
        None,
        None,
        "@import url(https://example\\2f4a8f.com\\\n/style.css);",
    );
    assert_import_dependency(
        input,
        &dependencies[2],
        "https://example\\2f4a8f.com\\\n/style.css",
        None,
        None,
        None,
        "@import url('https://example\\2f4a8f.com\\\n/style.css') /* */;",
    );
}

#[test]
fn unexpected_semicolon_in_supports() {
    let input = indoc! {r#"
        @import "style.css" supports(display: flex; display: grid);
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert_import_dependency(
        input,
        &dependencies[0],
        "style.css",
        None,
        None,
        Some(" supports(display: flex"),
        "@import \"style.css\" supports(display: flex;",
    );
    assert_warning(input, &warnings[0], ";");
}

#[test]
fn unexpected_semicolon_import_url_string() {
    let input = indoc! {r#"
        @import url("style.css";);
        @import url("style.css" layer;);
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(dependencies.is_empty());
    assert_warning(input, &warnings[0], ";");
    assert_warning(input, &warnings[1], ";");
}

#[test]
fn expected_before() {
    let input = indoc! {r#"
        @import layer supports(display: flex) "style.css";
        @import supports(display: flex) "style.css";
        @import layer "style.css";
        @import "style.css" supports(display: flex) layer;
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(dependencies.is_empty());
    assert_warning(input, &warnings[0], "\"style.css\"");
    assert_warning(input, &warnings[1], "\"style.css\"");
    assert_warning(input, &warnings[2], "\"style.css\"");
    assert_warning(input, &warnings[3], "layer");
}

#[test]
fn import_media() {
    let input = indoc! {r#"
        @import url("style.css") screen and (orientation: portrait);
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_import_dependency(
        input,
        &dependencies[0],
        "style.css",
        None,
        None,
        Some(" screen and (orientation: portrait)"),
        "@import url(\"style.css\") screen and (orientation: portrait);",
    );
}

#[test]
fn import_attributes() {
    let input = indoc! {r#"
        @import url("style.css") layer;
        @import url("style.css") supports();
        @import url("style.css") print;
        @import url("style.css") layer supports() /* comments */;
        @import url("style.css") layer(default) supports(not (display: grid) and (display: flex)) print, /* comments */ screen and (orientation: portrait);
    "#};
    let (dependencies, warnings) = collect_css_dependencies(input);
    assert!(warnings.is_empty());
    assert_import_dependency(
        input,
        &dependencies[0],
        "style.css",
        Some(""),
        None,
        None,
        "@import url(\"style.css\") layer;",
    );
    assert_import_dependency(
        input,
        &dependencies[1],
        "style.css",
        None,
        Some(""),
        None,
        "@import url(\"style.css\") supports();",
    );
    assert_import_dependency(
        input,
        &dependencies[2],
        "style.css",
        None,
        None,
        Some(" print"),
        "@import url(\"style.css\") print;",
    );
    assert_import_dependency(
        input,
        &dependencies[3],
        "style.css",
        Some(""),
        Some(""),
        None,
        "@import url(\"style.css\") layer supports() /* comments */;",
    );
    assert_import_dependency(
        input,
        &dependencies[4],
        "style.css",
        Some("default"),
        Some("not (display: grid) and (display: flex)"),
        Some(" print, /* comments */ screen and (orientation: portrait)"),
        "@import url(\"style.css\") layer(default) supports(not (display: grid) and (display: flex)) print, /* comments */ screen and (orientation: portrait);",
    );
}

#[test]
fn css_modules_pseudo_1() {
    let input = ".localA :global .global-b .global-c :local(.localD.localE) .global-d";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".localA", false);
    assert_replace_dependency(input, &dependencies[1], "", ":global ");
    assert_replace_dependency(input, &dependencies[2], "", ":local(");
    assert_local_ident_dependency(input, &dependencies[3], ".localD", true);
    assert_local_ident_dependency(input, &dependencies[4], ".localE", true);
    assert_replace_dependency(input, &dependencies[5], "", ")");
}

#[test]
fn css_modules_pseudo_2() {
    let input = indoc! {r#"
        :global .a :local .b :global .c {}
        .d {}
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_replace_dependency(input, &dependencies[0], "", ":global ");
    assert_replace_dependency(input, &dependencies[1], "", ":local ");
    assert_local_ident_dependency(input, &dependencies[2], ".b", true);
    assert_replace_dependency(input, &dependencies[3], "", ":global ");
    assert_local_ident_dependency(input, &dependencies[4], ".d", false);
    assert_eq!(dependencies.len(), 5);
}

#[test]
fn css_modules_pseudo_3() {
    let input = ".a:not(:global .b:not(.c:not(:global .d) .e) .f).g {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".a", false);
    assert_replace_dependency(input, &dependencies[1], "", ":global ");
    assert_replace_dependency(input, &dependencies[2], "", ":global ");
    assert_local_ident_dependency(input, &dependencies[3], ".g", false);
    assert_eq!(dependencies.len(), 4);
}

#[test]
fn css_modules_pseudo_4() {
    let input = ".a:not(:global .b:not(:local .c:not(:global .d) .e) .f).g {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".a", false);
    assert_replace_dependency(input, &dependencies[1], "", ":global ");
    assert_replace_dependency(input, &dependencies[2], "", ":local ");
    assert_local_ident_dependency(input, &dependencies[3], ".c", true);
    assert_replace_dependency(input, &dependencies[4], "", ":global ");
    assert_local_ident_dependency(input, &dependencies[5], ".e", true);
    assert_local_ident_dependency(input, &dependencies[6], ".g", false);
    assert_eq!(dependencies.len(), 7);
}

#[test]
fn css_modules_pseudo_5() {
    let input = ":global(.a, .b) {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_replace_dependency(input, &dependencies[0], "", ":global(");
    assert_replace_dependency(input, &dependencies[1], "", ")");
    assert_eq!(dependencies.len(), 2);
}

#[test]
fn css_modules_pseudo_6() {
    let input = ".a:local( .b ).c {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".a", false);
    assert_replace_dependency(input, &dependencies[1], "", ":local( ");
    assert_local_ident_dependency(input, &dependencies[2], ".b", true);
    assert_replace_dependency(input, &dependencies[3], "", " )");
    assert_local_ident_dependency(input, &dependencies[4], ".c", false);
    assert_eq!(dependencies.len(), 5);
}

#[test]
fn css_modules_missing_white_space_1() {
    let input = indoc! {r#"
        .a:global,:global .b {}
        .a{}:global .b{}
        :global .a {}
        .a:not(:global .b) {}
        .a:not(.b :global) {}
        .a :global,.b :global {}
        .a :global{}
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    // .a:global,:global .b {}
    assert_local_ident_dependency(input, &dependencies[0], ".a", false);
    assert_replace_dependency(input, &dependencies[1], "", ":global");
    assert_replace_dependency(input, &dependencies[2], "", ":global ");
    // .a{}:global .b{}
    assert_local_ident_dependency(input, &dependencies[3], ".a", false);
    assert_replace_dependency(input, &dependencies[4], "", ":global ");
    // :global .a {}
    assert_replace_dependency(input, &dependencies[5], "", ":global ");
    // .a:not(:global .b) {}
    assert_local_ident_dependency(input, &dependencies[6], ".a", false);
    assert_replace_dependency(input, &dependencies[7], "", ":global ");
    // .a:not(.b :global) {}
    assert_local_ident_dependency(input, &dependencies[8], ".a", false);
    assert_local_ident_dependency(input, &dependencies[9], ".b", false);
    assert_replace_dependency(input, &dependencies[10], "", ":global");
    // .a :global,.b :global {}
    assert_local_ident_dependency(input, &dependencies[11], ".a", false);
    assert_replace_dependency(input, &dependencies[12], "", ":global");
    assert_local_ident_dependency(input, &dependencies[13], ".b", false);
    assert_replace_dependency(input, &dependencies[14], "", ":global ");
    // .a :global{}
    assert_local_ident_dependency(input, &dependencies[15], ".a", false);
    assert_replace_dependency(input, &dependencies[16], "", ":global");
    assert_eq!(dependencies.len(), 17);
}

#[test]
fn css_modules_missing_white_space_2() {
    let input = ".a:not(.b:not(:global .c):local .d) {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert_warning(input, &warnings[0], ":local");
    assert_eq!(warnings.len(), 1);
    assert_local_ident_dependency(input, &dependencies[0], ".a", false);
    assert_local_ident_dependency(input, &dependencies[1], ".b", false);
    assert_replace_dependency(input, &dependencies[2], "", ":global ");
    assert_replace_dependency(input, &dependencies[3], "", ":local ");
    assert_local_ident_dependency(input, &dependencies[4], ".d", true);
    assert_eq!(dependencies.len(), 5);
}

#[test]
fn css_modules_nesting() {
    let input = indoc! {r#"
        .nested {
            .nested-nested {
                color: red;
            }
        }
        .nested-at-rule {
            @media screen {
                .nested-nested-at-rule-deep {
                    color: red;
                }
            }
        }
        :global .nested2 {
            .nested2-nested {
                color: red;
            }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".nested", false);
    assert_local_ident_dependency(input, &dependencies[1], ".nested-nested", false);
    assert_local_ident_dependency(input, &dependencies[2], ".nested-at-rule", false);
    assert_local_ident_dependency(
        input,
        &dependencies[3],
        ".nested-nested-at-rule-deep",
        false,
    );
    assert_replace_dependency(input, &dependencies[4], "", ":global ");
    assert_local_ident_dependency(input, &dependencies[5], ".nested2-nested", false);
    assert_eq!(dependencies.len(), 6);
}

#[test]
fn css_modules_local_var_unexpected() {
    let input = indoc! {r#"
        .vars {
            color: var(local-color);
        }
    "#};
    let (_, warnings) = collect_css_modules_dependencies(input);
    assert_warning(input, &warnings[0], "lo");
}

#[test]
fn css_modules_local_var() {
    let input = indoc! {r#"
        .vars {
            color: var(--local-color, red);
            --local-color: red;
        }
        .globalVars :global {
            color: var(--global-color);
            --global-color: red;
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".vars", false);
    assert_local_var_dependency(input, &dependencies[1], "local-color");
    assert_local_var_decl_dependency(input, &dependencies[2], "local-color");
    assert_local_ident_dependency(input, &dependencies[3], ".globalVars", false);
    assert_replace_dependency(input, &dependencies[4], "", ":global ");
}

#[test]
fn css_modules_local_var_minified_1() {
    let input = "body{margin:0;font-family:var(--bs-body-font-family);}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_var_dependency(input, &dependencies[0], "bs-body-font-family");
}

#[test]
fn css_modules_local_var_minified_2() {
    let input = ".table-primary{--bs-table-color:#000;--bs-table-border-color:#a6b5cc;color:var(--bs-table-color);border-color:var(--bs-table-border-color)}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".table-primary", false);
    assert_local_var_decl_dependency(input, &dependencies[1], "bs-table-color");
    assert_local_var_decl_dependency(input, &dependencies[2], "bs-table-border-color");
    assert_local_var_dependency(input, &dependencies[3], "bs-table-color");
    assert_local_var_dependency(input, &dependencies[4], "bs-table-border-color");
}

#[test]
fn css_modules_property() {
    let input = indoc! {r#"
        @property --my-color {
            syntax: "<color>";
            inherits: false;
            initial-value: #c0ffee;
        }
        .class {
            color: var(--my-color);
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_property_decl_dependency(input, &dependencies[0], "my-color");
    assert_local_ident_dependency(input, &dependencies[1], ".class", false);
    assert_local_var_dependency(input, &dependencies[2], "my-color");
}

#[test]
fn css_modules_keyframes_unexpected() {
    let input = indoc! {r#"
        @keyframes $aaa {
            0% { color: var(--theme-color1); }
            100% { color: var(--theme-color2); }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert_warning(input, &warnings[0], "$a");
    assert_eq!(warnings.len(), 1);
    assert_local_var_dependency(input, &dependencies[0], "theme-color1");
    assert_local_var_dependency(input, &dependencies[1], "theme-color2");
    assert_eq!(dependencies.len(), 2);
}

#[test]
fn css_modules_keyframes_1() {
    let input = indoc! {r#"
        @keyframes localkeyframes {
            0% { color: var(--theme-color1); }
            100% { color: var(--theme-color2); }
        }
        @keyframes localkeyframes2 {
            0% { left: 0; }
            100% { left: 100px; }
        }
        .animation {
            animation-name: localkeyframes;
            animation: 3s ease-in 1s 2 reverse both paused localkeyframes, localkeyframes2;
            --theme-color1: red;
            --theme-color2: blue;
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_keyframes_decl_dependency(input, &dependencies[0], "localkeyframes");
    assert_local_var_dependency(input, &dependencies[1], "theme-color1");
    assert_local_var_dependency(input, &dependencies[2], "theme-color2");
    assert_local_keyframes_decl_dependency(input, &dependencies[3], "localkeyframes2");
    assert_local_ident_dependency(input, &dependencies[4], ".animation", false);
    assert_local_keyframes_dependency(input, &dependencies[5], "localkeyframes");
    assert_local_keyframes_dependency(input, &dependencies[6], "localkeyframes");
    assert_local_keyframes_dependency(input, &dependencies[7], "localkeyframes2");
    assert_local_var_decl_dependency(input, &dependencies[8], "theme-color1");
    assert_local_var_decl_dependency(input, &dependencies[9], "theme-color2");
    assert_eq!(dependencies.len(), 10);
}

#[test]
fn css_modules_keyframes_2() {
    let input = indoc! {r#"
        @keyframes slidein {
            from { width: 300%; }
            to { width: 100%; }
        }
        .class {
            --animation-name: slidein;
            animation:
                var(--animation-name) 3s,
                3s linear 1s infinite running env(slidein),
                3s linear env(slidein, var(--baz)) infinite running slidein;
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_keyframes_decl_dependency(input, &dependencies[0], "slidein");
    assert_local_ident_dependency(input, &dependencies[1], ".class", false);
    assert_local_var_decl_dependency(input, &dependencies[2], "animation-name");
    assert_local_var_dependency(input, &dependencies[3], "animation-name");
    assert_local_var_dependency(input, &dependencies[4], "baz");
    assert_local_keyframes_dependency(input, &dependencies[5], "slidein");
    assert_eq!(dependencies.len(), 6);
}

#[test]
fn css_modules_keyframes_3() {
    let input = "@keyframes :local(foo) {}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_replace_dependency(input, &dependencies[0], "", ":local(");
    assert_local_keyframes_decl_dependency(input, &dependencies[1], "foo");
    assert_replace_dependency(input, &dependencies[2], "", ")");
    assert_eq!(dependencies.len(), 3);
}

#[test]
fn css_modules_at_rule_1() {
    let input = indoc! {r#"
        @layer framework.container {
            .class {
                color: red;
            }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".class", false);
    assert_eq!(dependencies.len(), 1);
}

#[test]
fn css_modules_at_rule_2() {
    let input = indoc! {r#"
        @page {
            .class {
                color: red;
            }
        }
        @page :left, :top {
            .class2 {
                color: red;
            }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".class", false);
    assert_local_ident_dependency(input, &dependencies[1], ".class2", false);
    assert_eq!(dependencies.len(), 2);
}

#[test]
fn css_modules_at_rule_3() {
    let input = indoc! {r#"
        .article-body {
            color: red;
        }
        @scope (.article-body) to (figure) {
            .img {
                background-color: goldenrod;
            }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".article-body", false);
    assert_local_ident_dependency(input, &dependencies[1], ".article-body", false);
    assert_local_ident_dependency(input, &dependencies[2], ".img", false);
    assert_eq!(dependencies.len(), 3);
}

#[test]
fn css_modules_composes_1() {
    let input = indoc! {r#"
        .exportName {
            composes: importName from "path/library.css", beforeName from global, importName secondImport from global, firstImport secondImport from "path/library.css";
            other: rule;
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".exportName", false);
    assert_composes_dependency(
        input,
        &dependencies[1],
        "importName",
        Some("\"path/library.css\""),
    );
    assert_composes_dependency(input, &dependencies[2], "beforeName", Some("global"));
    assert_composes_dependency(
        input,
        &dependencies[3],
        "importName secondImport",
        Some("global"),
    );
    assert_composes_dependency(
        input,
        &dependencies[4],
        "firstImport secondImport",
        Some("\"path/library.css\""),
    );
    assert_replace_dependency(
        input,
        &dependencies[5],
        "",
        r#"composes: importName from "path/library.css", beforeName from global, importName secondImport from global, firstImport secondImport from "path/library.css";"#,
    );
    assert_eq!(dependencies.len(), 6);
}

#[test]
fn css_modules_composes_2() {
    let input = indoc! {r#"
        .duplicate {
            composes: a from "./aa.css", b from "./bb.css", c from './cc.css', a from './aa.css', c from './cc.css'
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".duplicate", false);
    assert_composes_dependency(input, &dependencies[1], "a", Some("\"./aa.css\""));
    assert_composes_dependency(input, &dependencies[2], "b", Some("\"./bb.css\""));
    assert_composes_dependency(input, &dependencies[3], "c", Some("'./cc.css'"));
    assert_composes_dependency(input, &dependencies[4], "a", Some("'./aa.css'"));
    assert_composes_dependency(input, &dependencies[5], "c", Some("'./cc.css'"));
    assert_replace_dependency(
        input,
        &dependencies[6],
        "",
        r#"composes: a from "./aa.css", b from "./bb.css", c from './cc.css', a from './aa.css', c from './cc.css'"#,
    );
    assert_eq!(dependencies.len(), 7);
}

#[test]
fn css_modules_composes_3() {
    let input = indoc! {r#"
        .spaces {
            composes: importName importName2 from "path/library.css", importName3 importName4 from "path/library.css";
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".spaces", false);
    assert_composes_dependency(
        input,
        &dependencies[1],
        "importName importName2",
        Some("\"path/library.css\""),
    );
    assert_composes_dependency(
        input,
        &dependencies[2],
        "importName3 importName4",
        Some("\"path/library.css\""),
    );
    assert_replace_dependency(
        input,
        &dependencies[3],
        "",
        r#"composes: importName importName2 from "path/library.css", importName3 importName4 from "path/library.css";"#,
    );
    assert_eq!(dependencies.len(), 4);
}

#[test]
fn css_modules_composes_4() {
    let input = indoc! {r#"
        .unknown {
            composes: foo bar, baz;
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".unknown", false);
    assert_composes_dependency(input, &dependencies[1], "foo bar", None);
    assert_composes_dependency(input, &dependencies[2], "baz", None);
    assert_replace_dependency(input, &dependencies[3], "", r#"composes: foo bar, baz;"#);
    assert_eq!(dependencies.len(), 4);
}

#[test]
fn css_modules_composes_5() {
    let input = indoc! {r#"
        .mixed {
            composes: foo bar, baz, importName importName2 from "path/library.css"
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], ".mixed", false);
    assert_composes_dependency(input, &dependencies[1], "foo bar", None);
    assert_composes_dependency(input, &dependencies[2], "baz", None);
    assert_composes_dependency(
        input,
        &dependencies[3],
        "importName importName2",
        Some("\"path/library.css\""),
    );
    assert_replace_dependency(
        input,
        &dependencies[4],
        "",
        r#"composes: foo bar, baz, importName importName2 from "path/library.css""#,
    );
    assert_eq!(dependencies.len(), 5);
}

#[test]
fn icss_export_unexpected() {
    let input = ":export {\n/sl/ash;";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert_warning(input, &warnings[0], ";");
    assert_eq!(warnings.len(), 1);
    assert_replace_dependency(input, &dependencies[0], "", ":export {\n/sl/ash");
    assert_eq!(dependencies.len(), 1);
}

#[test]
fn icss_import() {
    let input = indoc! {r#"
        :import(col.ors-2) {}
        :import("./colors.css") { i__blue: blue; i__red: red; }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_icss_import_from_dependency(input, &dependencies[0], "col.ors-2");
    assert_replace_dependency(input, &dependencies[1], "", ":import(col.ors-2) {}");
    assert_icss_import_from_dependency(input, &dependencies[2], "\"./colors.css\"");
    assert_icss_import_value_dependency(input, &dependencies[3], "i__blue", "blue");
    assert_icss_import_value_dependency(input, &dependencies[4], "i__red", "red");
    assert_replace_dependency(
        input,
        &dependencies[5],
        "",
        ":import(\"./colors.css\") { i__blue: blue; i__red: red; }",
    );
    assert_eq!(dependencies.len(), 6);
}

#[test]
fn icss_export() {
    let input = indoc! {r#"
        :export {
            a: a;
        }
        :export {
            abc: a b c;
            comments: abc/****/   /* hello world *//****/   def
        }
        :export{default:default}
        :export { $: abc; }
        :export { white space: a b c; }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_icss_export_value_dependency(input, &dependencies[0], "a", "a");
    assert_replace_dependency(
        input,
        &dependencies[1],
        "",
        indoc! {r#":export {
            a: a;
        }"#},
    );
    assert_icss_export_value_dependency(input, &dependencies[2], "abc", "a b c");
    assert_icss_export_value_dependency(
        input,
        &dependencies[3],
        "comments",
        "abc/****/   /* hello world *//****/   def",
    );
    assert_replace_dependency(
        input,
        &dependencies[4],
        "",
        indoc! {r#":export {
            abc: a b c;
            comments: abc/****/   /* hello world *//****/   def
        }"#},
    );
    assert_icss_export_value_dependency(input, &dependencies[5], "default", "default");
    assert_replace_dependency(input, &dependencies[6], "", ":export{default:default}");
    assert_icss_export_value_dependency(input, &dependencies[7], "$", "abc");
    assert_replace_dependency(input, &dependencies[8], "", ":export { $: abc; }");
    assert_icss_export_value_dependency(input, &dependencies[9], "white space", "a b c");
    assert_replace_dependency(
        input,
        &dependencies[10],
        "",
        ":export { white space: a b c; }",
    );
    assert_eq!(dependencies.len(), 11);
}
