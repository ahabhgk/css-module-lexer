use codspeed_criterion_compat::*;
use css_module_lexer::collect_css_modules_dependencies;

const BOOTSTRAP: &str = include_str!("../fixtures/bootstrap.min.css");

fn benchmark(c: &mut Criterion) {
    c.bench_function("bootstrap", |b| {
        b.iter(|| collect_css_modules_dependencies(black_box(BOOTSTRAP)))
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
