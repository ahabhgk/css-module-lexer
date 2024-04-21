use css_module_lexer::{
    CollectDependencies, Collection, Dependency, Lexer, UrlKind, Visitor, Warning,
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

fn assert_url_dependency(
    lexer: &Lexer,
    dependency: &Dependency,
    request: &str,
    kind: UrlKind,
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
    assert_eq!(lexer.slice(range.start, range.end).unwrap(), range_content);
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
fn url() {
    let mut v = CollectDependencies::default();
    let mut l = Lexer::from(indoc! {r#"
        body {
            background: url(
                https://example\2f4a8f.com\
        /image.png
            )
        }
    "#});
    l.lex(&mut v);
    let Collection {
        dependencies,
        warnings,
    } = v.into();
    assert!(warnings.is_empty());
    assert_url_dependency(
        &l,
        &dependencies[0],
        "https://example\\2f4a8f.com\\\n/image.png",
        UrlKind::Url,
        "url(\n        https://example\\2f4a8f.com\\\n/image.png\n    )",
    );
}

#[test]
fn duplicate_url() {
    let mut v = CollectDependencies::default();
    let mut l = Lexer::from(indoc! {r#"
        @import url(./a.css) url(./a.css);
    "#});
    l.lex(&mut v);
    let Collection {
        dependencies,
        warnings,
    } = v.into();
    assert!(dependencies.is_empty());
    let Warning::DuplicateUrl(range) = &warnings[0] else {
        return assert!(false);
    };
    assert_eq!(
        l.slice(range.start, range.end).unwrap(),
        "@import url(./a.css) url(./a.css)"
    );
}

#[test]
fn not_preceded_at_import() {
    let mut v = CollectDependencies::default();
    let mut l = Lexer::from(indoc! {r#"
        body {}
        @import url(./a.css);
    "#});
    l.lex(&mut v);
    let Collection {
        dependencies,
        warnings,
    } = v.into();
    assert!(dependencies.is_empty());
    let Warning::NotPrecededAtImport(range) = &warnings[0] else {
        return assert!(false);
    };
    assert_eq!(l.slice(range.start, range.end).unwrap(), "@import");
}

#[test]
fn url_string() {
    let mut v = CollectDependencies::default();
    let mut l = Lexer::from(indoc! {r#"
        body {
            a: url("https://example\2f4a8f.com\
            /image.png");
            b: image-set(
                "image1.png" 1x,
                "image2.png" 2x
            );
            c: image-set(
                url("image1.avif") type("image/avif"),
                url("image2.jpg") type("image/jpeg")
            );
        }
    "#});
    l.lex(&mut v);
    let Collection {
        dependencies,
        warnings,
    } = v.into();
    assert!(warnings.is_empty());
    assert_url_dependency(
        &l,
        &dependencies[0],
        "https://example\\2f4a8f.com\\\n    /image.png",
        UrlKind::Url,
        "\"https://example\\2f4a8f.com\\\n    /image.png\"",
    );
    assert_url_dependency(
        &l,
        &dependencies[1],
        "image1.png",
        UrlKind::String,
        "\"image1.png\"",
    );
    assert_url_dependency(
        &l,
        &dependencies[2],
        "image2.png",
        UrlKind::String,
        "\"image2.png\"",
    );
    assert_url_dependency(
        &l,
        &dependencies[3],
        "image1.avif",
        UrlKind::Url,
        "\"image1.avif\"",
    );
    assert_url_dependency(
        &l,
        &dependencies[4],
        "image2.jpg",
        UrlKind::Url,
        "\"image2.jpg\"",
    );
}
