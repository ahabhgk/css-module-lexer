use codspeed_criterion_compat::*;
use css_module_lexer::{Lexer, Mode, TokenKind, collect_dependencies, lex_dependencies};

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
const RUN_MICRO_BENCHES: &str = "CSS_MODULE_LEXER_RUN_MICRO_BENCHES";

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

fn sink_dependencies(input: &str, mode: Mode) {
    lex_dependencies(input, mode, |_| {}, |_| {});
}

fn micro_benchmarks_enabled() -> bool {
    matches!(
        std::env::var(RUN_MICRO_BENCHES).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
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

    if !micro_benchmarks_enabled() {
        return;
    }

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
