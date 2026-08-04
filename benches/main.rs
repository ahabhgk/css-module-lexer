use codspeed_criterion_compat::*;
use css_module_lexer::{
    collect_dependencies, lex_dependencies, Lexer, Mode, Pos, TokenKind, Visitor,
};

const BOOTSTRAP: &str = include_str!("../fixtures/bootstrap.min.css");
const RSPACK_CSS_MODULES: &str = include_str!("../fixtures/rspack-css-modules.css");
const COMMENT_HEAVY_VALUE: &str = r#"
    @value /* one */ red /* two */ from /* three */ "./colors.css";
    @value blue /* four */ as /* five */ sky, green from "./palette.css";
    :local/**/(.button/**/, .icon/**/:global(.glyph)) { color: red; }
"#;
const NESTED_SELECTOR: &str = r#"
    .card {
        & .title, :global(.icon) { color: red; }
        &:hover { .label { color: blue; } }
    }
"#;
const COMMENT_SEPARATED_MODE: &str = r#"
    :local/**/.foo { color: red; }
    :global/**/#bar { color: blue; }
"#;
const ESCAPE_HEAVY_IDENTIFIER: &str = r#"
    .a\31 23\20name, #\66 00d { --custom\2d property: url(https:\2f\2fexample.com\2fimage.png); }
"#;

fn scan_tokens(input: &str) -> usize {
    let mut lexer = Lexer::new(input);
    let mut count = 0;
    loop {
        let token = lexer.next_token();
        count += 1;
        if token.kind == TokenKind::Eof {
            return count;
        }
    }
}

struct NoopVisitor;

impl<'s> Visitor<'s> for NoopVisitor {
    fn function(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn ident(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn url(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn string(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn is_selector(&mut self, _: &mut Lexer<'s>) -> Option<bool> {
        Some(true)
    }

    fn id(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn left_parenthesis(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn right_parenthesis(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn comma(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn class(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn pseudo_function(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn pseudo_class(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn semicolon(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn at_keyword(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn left_curly_bracket(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }

    fn right_curly_bracket(&mut self, _: &mut Lexer<'s>, _: Pos, _: Pos) -> Option<()> {
        Some(())
    }
}

fn scan_legacy(input: &str) -> Pos {
    let mut lexer = Lexer::new(input);
    let mut visitor = NoopVisitor;
    lexer.lex(&mut visitor);
    lexer.cur_pos().unwrap_or_default()
}

fn sink_dependencies(input: &str, mode: Mode) {
    lex_dependencies(input, mode, |_| {}, |_| {});
}

fn benchmark(c: &mut Criterion) {
    c.bench_function("bootstrap", |b| {
        b.iter(|| collect_dependencies(black_box(BOOTSTRAP), Mode::Local))
    });

    let mut group = c.benchmark_group("rspack_css_modules");
    for (name, mode) in [
        ("local", Mode::Local),
        ("global", Mode::Global),
        ("pure", Mode::Pure),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| black_box(collect_dependencies(black_box(RSPACK_CSS_MODULES), mode)))
        });
    }
    group.finish();

    let mut tokenizer = c.benchmark_group("tokenizer");
    for (name, input) in [
        ("comment_heavy_value", COMMENT_HEAVY_VALUE),
        ("nested_selector", NESTED_SELECTOR),
        ("comment_separated_mode", COMMENT_SEPARATED_MODE),
        ("escape_heavy_identifier", ESCAPE_HEAVY_IDENTIFIER),
        ("large_css", BOOTSTRAP),
    ] {
        tokenizer.bench_function(name, |b| {
            b.iter(|| black_box(scan_tokens(black_box(input))))
        });
    }
    tokenizer.bench_function("legacy_scanner", |b| {
        b.iter(|| black_box(scan_legacy(black_box(BOOTSTRAP))))
    });
    tokenizer.finish();

    let mut sink = c.benchmark_group("dependency_sink");
    for (name, input, mode) in [
        ("comment_heavy_value", COMMENT_HEAVY_VALUE, Mode::Local),
        ("nested_selector", NESTED_SELECTOR, Mode::Local),
        (
            "comment_separated_mode",
            COMMENT_SEPARATED_MODE,
            Mode::Local,
        ),
        (
            "escape_heavy_identifier",
            ESCAPE_HEAVY_IDENTIFIER,
            Mode::Local,
        ),
        ("large_css", BOOTSTRAP, Mode::Local),
    ] {
        sink.bench_function(name, |b| {
            b.iter(|| sink_dependencies(black_box(input), mode))
        });
    }
    sink.finish();
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
