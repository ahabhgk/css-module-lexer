use codspeed_criterion_compat::*;
use css_module_lexer::collect_dependencies;
use css_module_lexer::Mode;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const BOOTSTRAP: &str = include_str!("../fixtures/bootstrap.min.css");
const ITERATIONS: usize = 100;

fn benchmark(c: &mut Criterion) {
    c.bench_function("bootstrap", |b| {
        b.iter(|| {
            for _ in 0..ITERATIONS {
                black_box(collect_dependencies(black_box(BOOTSTRAP), Mode::Local));
            }
        })
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
