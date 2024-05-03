use css_module_lexer::collect_css_dependencies;
use css_module_lexer::collect_css_modules_dependencies;
use css_module_lexer::Dependency;
use css_module_lexer::Lexer;
use css_module_lexer::Pos;
use css_module_lexer::Range;
use css_module_lexer::UrlRangeKind;
use css_module_lexer::Visitor;
use css_module_lexer::Warning;
use indoc::indoc;

fn assert_lexer_state(
    lexer: &Lexer,
    cur: Option<char>,
    cur_pos: Option<Pos>,
    peek: Option<char>,
    peek_pos: Option<Pos>,
    peek2: Option<char>,
    peek2_pos: Option<Pos>,
) {
    assert_eq!(lexer.cur(), cur);
    assert_eq!(lexer.cur_pos(), cur_pos);
    assert_eq!(lexer.peek(), peek);
    assert_eq!(lexer.peek_pos(), peek_pos);
    assert_eq!(lexer.peek2(), peek2);
    assert_eq!(lexer.peek2_pos(), peek2_pos);
}

#[derive(Default)]
struct Snapshot {
    results: Vec<(String, String)>,
}

impl Snapshot {
    pub fn add(&mut self, key: &str, value: &str) {
        self.results.push((key.to_string(), value.to_string()))
    }

    pub fn snapshot(&self) -> String {
        self.results
            .iter()
            .map(|(k, v)| format!("{k}: {v}\n"))
            .collect::<String>()
    }
}

impl Visitor<'_> for Snapshot {
    fn function(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("function", lexer.slice(start, end)?);
        Some(())
    }

    fn ident(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("ident", lexer.slice(start, end)?);
        Some(())
    }

    fn url(
        &mut self,
        lexer: &mut Lexer,
        _: Pos,
        _: Pos,
        content_start: Pos,
        content_end: Pos,
    ) -> Option<()> {
        self.add("url", lexer.slice(content_start, content_end)?);
        Some(())
    }

    fn string(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("string", lexer.slice(start, end)?);
        Some(())
    }

    fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
        Some(true)
    }

    fn id(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("id", lexer.slice(start, end)?);
        Some(())
    }

    fn left_parenthesis(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("left_parenthesis", lexer.slice(start, end)?);
        Some(())
    }

    fn right_parenthesis(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("right_parenthesis", lexer.slice(start, end)?);
        Some(())
    }

    fn comma(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("comma", lexer.slice(start, end)?);
        Some(())
    }

    fn class(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("class", lexer.slice(start, end)?);
        Some(())
    }

    fn pseudo_function(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("pseudo_function", lexer.slice(start, end)?);
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("pseudo_class", lexer.slice(start, end)?);
        Some(())
    }

    fn semicolon(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("semicolon", lexer.slice(start, end)?);
        Some(())
    }

    fn at_keyword(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("at_keyword", lexer.slice(start, end)?);
        Some(())
    }

    fn left_curly_bracket(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("left_curly", lexer.slice(start, end)?);
        Some(())
    }

    fn right_curly_bracket(&mut self, lexer: &mut Lexer, start: Pos, end: Pos) -> Option<()> {
        self.add("right_curly", lexer.slice(start, end)?);
        Some(())
    }
}

fn assert_lexer_snapshot(input: &str, snapshot: &str) {
    let mut s = Snapshot::default();
    let mut l = Lexer::from(input);
    l.lex(&mut s);
    assert!(l.cur().is_none());
    assert_eq!(s.snapshot(), snapshot);
}

fn slice_range<'a>(input: &'a str, range: &Range) -> Option<&'a str> {
    input.get(range.start as usize..range.end as usize)
}

fn assert_warning(input: &str, warning: &Warning, range_content: &str) {
    match warning {
        Warning::Unexpected { range, .. }
        | Warning::DuplicateUrl { range, .. }
        | Warning::NamespaceNotSupportedInBundledCss { range }
        | Warning::NotPrecededAtImport { range }
        | Warning::ExpectedUrl { range, .. }
        | Warning::ExpectedUrlBefore { range, .. }
        | Warning::ExpectedLayerBefore { range, .. } => {
            assert_eq!(slice_range(input, range).unwrap(), range_content);
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
    assert_eq!(slice_range(input, range).unwrap(), range_content);
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
    assert_eq!(slice_range(input, range).unwrap(), range_content);
}

fn assert_local_ident_dependency(input: &str, dependency: &Dependency, name: &str) {
    let Dependency::LocalIdent {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(slice_range(input, range).unwrap(), name);
}

fn assert_local_var_dependency(
    input: &str,
    dependency: &Dependency,
    name: &str,
    range_content: &str,
) {
    let Dependency::LocalVar {
        name: actual_name,
        range,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(slice_range(input, range).unwrap(), range_content);
}

fn assert_local_var_decl_dependency(
    input: &str,
    dependency: &Dependency,
    name: &str,
    range_content: &str,
) {
    let Dependency::LocalVarDecl {
        range,
        name: actual_name,
    } = dependency
    else {
        return assert!(false);
    };
    assert_eq!(*actual_name, name);
    assert_eq!(slice_range(input, range).unwrap(), range_content);
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
    assert_eq!(slice_range(input, range).unwrap(), range_content);
}

fn assert_icss_export_dependency(_input: &str, dependency: &Dependency, prop: &str, value: &str) {
    let Dependency::ICSSExport {
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
fn lexer_start() {
    let mut l = Lexer::from("");
    assert_lexer_state(&l, None, None, None, Some(0), None, None);
    assert_eq!(l.consume(), None);
    assert_lexer_state(&l, None, Some(0), None, None, None, None);
    assert_eq!(l.consume(), None);
    let mut l = Lexer::from("0壹👂삼");
    assert_lexer_state(&l, None, None, Some('0'), Some(0), Some('壹'), Some(1));
    assert_eq!(l.consume(), Some('0'));
    assert_lexer_state(
        &l,
        Some('0'),
        Some(0),
        Some('壹'),
        Some(1),
        Some('👂'),
        Some(4),
    );
    assert_eq!(l.consume(), Some('壹'));
    assert_lexer_state(
        &l,
        Some('壹'),
        Some(1),
        Some('👂'),
        Some(4),
        Some('삼'),
        Some(8),
    );
    assert_eq!(l.consume(), Some('👂'));
    assert_lexer_state(&l, Some('👂'), Some(4), Some('삼'), Some(8), None, Some(11));
    assert_eq!(l.consume(), Some('삼'));
    assert_lexer_state(&l, Some('삼'), Some(8), None, Some(11), None, None);
    assert_eq!(l.consume(), None);
    assert_lexer_state(&l, None, Some(11), None, None, None, None);
    assert_eq!(l.consume(), None);
}

#[test]
fn parse_urls() {
    assert_lexer_snapshot(
        indoc! {r#"
            body {
                background: url(
                    https://example\2f4a8f.com\
            /image.png
                )
            }
            --element\ name.class\ name#_id {
                background: url(  "https://example.com/some url \"with\" 'spaces'.png"   )  url('https://example.com/\'"quotes"\'.png');
            }
        "#},
        indoc! {r#"
            ident: body
            left_curly: {
            ident: background
            url: https://example\2f4a8f.com\
            /image.png
            right_curly: }
            ident: --element\ name
            class: .class\ name
            id: #_id
            left_curly: {
            ident: background
            function: url(
            string: "https://example.com/some url \"with\" 'spaces'.png"
            right_parenthesis: )
            function: url(
            string: 'https://example.com/\'"quotes"\'.png'
            right_parenthesis: )
            semicolon: ;
            right_curly: }
        "#},
    );
}

#[test]
fn parse_pseudo_functions() {
    assert_lexer_snapshot(
        indoc! {r#"
            :local(.class#id, .class:not(*:hover)) { color: red; }
            :import(something from ":somewhere") {}
        "#},
        indoc! {r#"
            pseudo_function: :local(
            class: .class
            id: #id
            comma: ,
            class: .class
            pseudo_function: :not(
            pseudo_class: :hover
            right_parenthesis: )
            right_parenthesis: )
            left_curly: {
            ident: color
            ident: red
            semicolon: ;
            right_curly: }
            pseudo_function: :import(
            ident: something
            ident: from
            string: ":somewhere"
            right_parenthesis: )
            left_curly: {
            right_curly: }
        "#},
    );
}

#[test]
fn parse_at_rules() {
    assert_lexer_snapshot(
        indoc! {r#"
            @media (max-size: 100px) {
                @import "external.css";
                body { color: red; }
            }
        "#},
        indoc! {r#"
            at_keyword: @media
            left_parenthesis: (
            ident: max-size
            right_parenthesis: )
            left_curly: {
            at_keyword: @import
            string: "external.css"
            semicolon: ;
            ident: body
            left_curly: {
            ident: color
            ident: red
            semicolon: ;
            right_curly: }
            right_curly: }
        "#},
    );
}

#[test]
fn parse_escape() {
    assert_lexer_snapshot(
        indoc! {r#"
            body {
                a\
            a: \
            url(https://example\2f4a8f.com\
            /image.png)
                b: url(#\
            hash)
            }
        "#},
        indoc! {r#"
            ident: body
            left_curly: {
            ident: a\
            a
            url: https://example\2f4a8f.com\
            /image.png
            ident: b
            url: #\
            hash
            right_curly: }
        "#},
    );
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
fn css_modules_pseudo() {
    let input = ".localA :global .global-b .global-c :local(.localD.localE) .global-d";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], "localA");
    assert_replace_dependency(input, &dependencies[1], "", ":global ");
    assert_replace_dependency(input, &dependencies[2], "", ":local(");
    assert_local_ident_dependency(input, &dependencies[3], "localD");
    assert_local_ident_dependency(input, &dependencies[4], "localE");
    assert_replace_dependency(input, &dependencies[5], "", ")");
}

#[test]
fn css_modules_nesting() {
    let input = indoc! {r#"
        .first-nested {
            .first-nested-nested {
                color: red;
            }
        }
        .first-nested-at-rule {
            @media screen {
                .first-nested-nested-at-rule-deep {
                    color: red;
                }
            }
        }
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], "first-nested");
    assert_local_ident_dependency(input, &dependencies[1], "first-nested-nested");
    assert_local_ident_dependency(input, &dependencies[2], "first-nested-at-rule");
    assert_local_ident_dependency(input, &dependencies[3], "first-nested-nested-at-rule-deep");
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
    assert_local_ident_dependency(input, &dependencies[0], "vars");
    assert_local_var_dependency(input, &dependencies[1], "local-color", "--local-color");
    assert_local_var_decl_dependency(input, &dependencies[2], "local-color", "--local-color");
    assert_local_ident_dependency(input, &dependencies[3], "globalVars");
    assert_replace_dependency(input, &dependencies[4], "", ":global ");
    dbg!(dependencies, warnings);
}

#[test]
fn css_modules_local_var_minified_1() {
    let input = "body{margin:0;font-family:var(--bs-body-font-family);}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_var_dependency(
        input,
        &dependencies[0],
        "bs-body-font-family",
        "--bs-body-font-family",
    );
}

#[test]
fn css_modules_local_var_minified_2() {
    let input = ".table-primary{--bs-table-color:#000;--bs-table-border-color:#a6b5cc;color:var(--bs-table-color);border-color:var(--bs-table-border-color)}";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_local_ident_dependency(input, &dependencies[0], "table-primary");
    assert_local_var_decl_dependency(
        input,
        &dependencies[1],
        "bs-table-color",
        "--bs-table-color",
    );
    assert_local_var_decl_dependency(
        input,
        &dependencies[2],
        "bs-table-border-color",
        "--bs-table-border-color",
    );
    assert_local_var_dependency(
        input,
        &dependencies[3],
        "bs-table-color",
        "--bs-table-color",
    );
    assert_local_var_dependency(
        input,
        &dependencies[4],
        "bs-table-border-color",
        "--bs-table-border-color",
    );
}

#[test]
fn icss_export_unexpected() {
    let input = ":export {\n/sl/ash;";
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert_warning(input, &warnings[0], ";");
    assert_replace_dependency(input, &dependencies[0], "", ":export {\n/sl/ash");
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

        :export


        {


            white space

            :

            abc
            def

        }

        :export{default:default}
    "#};
    let (dependencies, warnings) = collect_css_modules_dependencies(input);
    assert!(warnings.is_empty());
    assert_icss_export_dependency(input, &dependencies[0], "a", "a");
    assert_replace_dependency(
        input,
        &dependencies[1],
        "",
        indoc! {r#":export {
            a: a;
        }"#},
    );
    assert_icss_export_dependency(input, &dependencies[2], "abc", "a b c");
    assert_icss_export_dependency(
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
    assert_icss_export_dependency(input, &dependencies[5], "white space", "abc\n    def");
    assert_replace_dependency(
        input,
        &dependencies[6],
        "",
        indoc! {r#":export


        {


            white space

            :

            abc
            def

        }"#},
    );
    assert_icss_export_dependency(input, &dependencies[7], "default", "default");
    assert_replace_dependency(input, &dependencies[8], "", ":export{default:default}");
}
