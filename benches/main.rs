use codspeed_criterion_compat::*;
use css_module_lexer::collect_dependencies;
use css_module_lexer::Mode;

const BOOTSTRAP: &str = include_str!("../fixtures/bootstrap.min.css");

fn benchmark(c: &mut Criterion) {
    c.bench_function("bootstrap", |b| {
        b.iter(|| collect_dependencies(black_box(BOOTSTRAP), Mode::Local))
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
