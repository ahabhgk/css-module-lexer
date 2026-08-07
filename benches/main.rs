use codspeed_criterion_compat::*;
use css_module_lexer::collect_dependencies;
use css_module_lexer::Mode;

const BOOTSTRAP: &str = include_str!("../fixtures/bootstrap.min.css");
const RSPACK_CSS_MODULES: &str = include_str!("../fixtures/rspack-css-modules.css");

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
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
