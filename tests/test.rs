use css_module_lexer::{
    collect_css_dependencies, Dependency, Lexer, UrlRangeKind, Visitor, Warning,
};
use indoc::indoc;

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
    fn function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("function", lexer.slice(start, end)?);
        Some(())
    }

    fn ident(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("ident", lexer.slice(start, end)?);
        Some(())
    }

    fn url(
        &mut self,
        lexer: &mut Lexer,
        _: usize,
        _: usize,
        content_start: usize,
        content_end: usize,
    ) -> Option<()> {
        self.add("url", lexer.slice(content_start, content_end)?);
        Some(())
    }

    fn string(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("string", lexer.slice(start, end)?);
        Some(())
    }

    fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
        Some(true)
    }

    fn id(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("id", lexer.slice(start, end)?);
        Some(())
    }

    fn left_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("left_parenthesis", lexer.slice(start, end)?);
        Some(())
    }

    fn right_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("right_parenthesis", lexer.slice(start, end)?);
        Some(())
    }

    fn comma(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("comma", lexer.slice(start, end)?);
        Some(())
    }

    fn class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("class", lexer.slice(start, end)?);
        Some(())
    }

    fn pseudo_function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("pseudo_function", lexer.slice(start, end)?);
        Some(())
    }

    fn pseudo_class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("pseudo_class", lexer.slice(start, end)?);
        Some(())
    }

    fn semicolon(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("semicolon", lexer.slice(start, end)?);
        Some(())
    }

    fn at_keyword(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("at_keyword", lexer.slice(start, end)?);
        Some(())
    }

    fn left_curly_bracket(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("left_curly", lexer.slice(start, end)?);
        Some(())
    }

    fn right_curly_bracket(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
        self.add("right_curly", lexer.slice(start, end)?);
        Some(())
    }
}

fn assert_warning(input: &str, warning: &Warning, range_content: &str) {
    match warning {
        Warning::DuplicateUrl(range)
        | Warning::NamespaceNotSupportedInBundledCss(range)
        | Warning::NotPrecededAtImport(range)
        | Warning::ExpectedUrl(range) => {
            assert_eq!(input.get(range.start..range.end).unwrap(), range_content);
        }
    }
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
    assert_eq!(input.get(range.start..range.end).unwrap(), range_content);
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
    assert_eq!(input.get(range.start..range.end).unwrap(), range_content);
}

#[test]
fn parse_urls() {
    let mut s = Snapshot::default();
    let mut l = Lexer::from(indoc! {r#"
        body {
            background: url(
                https://example\2f4a8f.com\
        /image.png
            )
        }
        --element\ name.class\ name#_id {
            background: url(  "https://example.com/some url \"with\" 'spaces'.png"   )  url('https://example.com/\'"quotes"\'.png');
        }
    "#});
    l.lex(&mut s);
    assert!(l.cur().is_none());
    assert_eq!(
        s.snapshot(),
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
        "#}
    );
}

#[test]
fn parse_pseudo_functions() {
    let mut s = Snapshot::default();
    let mut l = Lexer::from(indoc! {r#"
        :local(.class#id, .class:not(*:hover)) { color: red; }
        :import(something from ":somewhere") {}
    "#});
    l.lex(&mut s);
    assert!(l.cur().is_none());
    assert_eq!(
        s.snapshot(),
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
        "#}
    );
}

#[test]
fn parse_at_rules() {
    let mut s = Snapshot::default();
    let mut l = Lexer::from(indoc! {r#"
        @media (max-size: 100px) {
            @import "external.css";
            body { color: red; }
        }
    "#});
    l.lex(&mut s);
    assert!(l.cur().is_none());
    println!("{}", s.snapshot());
    assert_eq!(
        s.snapshot(),
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
        "#}
    );
}

#[test]
fn parse_escape() {
    let mut s = Snapshot::default();
    let mut l = Lexer::from(indoc! {r#"
        body {
            a\
        a: \
        url(https://example\2f4a8f.com\
        /image.png)
            b: url(#\
        hash)
        }
    "#});
    l.lex(&mut s);
    assert!(l.cur().is_none());
    assert_eq!(
        s.snapshot(),
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
        "#}
    );
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
fn empty() {
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
