# Benchmark baseline

Captured on 2026-08-04 on the same macOS host with the release benchmark:

```text
cargo bench --bench main -- --noplot
```

That command runs only the bootstrap and Rspack baseline groups. The focused
micro benchmarks are opt-in so the default CodSpeed CI job stays on the stable
baseline:

```text
CSS_MODULE_LEXER_RUN_MICRO_BENCHES=1 cargo bench --bench main -- --noplot
```

The three values in each cell are Criterion's lower bound, median, and upper
bound. The reference captures were taken before the tokenizer refactor; small
differences between runs are expected because these are wall-clock
microbenchmarks.

| case | pre-refactor branch | main snapshot | refactored |
| --- | ---: | ---: | ---: |
| bootstrap | 874.72 µs | 891.35 µs | 802.69–806.41–810.83 µs |
| Rspack local | 242.53 µs | 265.36 µs | 260.07–260.34–260.70 µs |
| Rspack global | 204.00 µs | 253.10 µs | 232.57–233.17–233.98 µs |
| Rspack pure | 253.57 µs | 269.19 µs | 265.14–266.74–268.69 µs |

The refactored benchmark also contains independent tokenizer and semantic-sink
cases. Their final medians were:

| case | tokenizer | dependency sink |
| --- | ---: | ---: |
| comment-heavy `@value` / ICSS | 215.95 ns | 782.88 ns |
| nested selector | 106.94 ns | 495.87 ns |
| comment-separated mode | 71.10 ns | 370.15 ns |
| escape-heavy identifier | 91.63 ns | 327.15 ns |
| large CSS | 252.45 µs | 828.78 µs |

The legacy scanner benchmark was removed together with the incompatible
`Visitor` API. The focused cases are defined in [`benches/main.rs`](benches/main.rs).

Validation for this change:

```text
cargo test --all-targets --quiet   # 13 unit + 3 integration + 281 fixture tests
cargo clippy --all-targets -- -D warnings
```
