# Benchmark baseline

Captured on 2026-08-04 on the same macOS host with the release benchmark:

```text
cargo bench --bench main -- --noplot
```

The three values in each cell are Criterion's lower bound, median, and upper
bound. The reference captures were taken before the tokenizer refactor; small
differences between runs are expected because these are wall-clock
microbenchmarks.

| case | pre-refactor branch | main snapshot | refactored |
| --- | ---: | ---: | ---: |
| bootstrap | 874.72 µs | 891.35 µs | 951.49–963.32–975.75 µs |
| Rspack local | 242.53 µs | 265.36 µs | 310.28–312.64–315.25 µs |
| Rspack global | 204.00 µs | 253.10 µs | 287.60–290.07–292.79 µs |
| Rspack pure | 253.57 µs | 269.19 µs | 311.79–312.81–313.96 µs |

The refactored benchmark also contains independent tokenizer and semantic-sink
cases. Their final medians were:

| case | tokenizer | dependency sink |
| --- | ---: | ---: |
| comment-heavy `@value` / ICSS | 266.68 ns | 1.0256 µs |
| nested selector | 172.27 ns | 404.88 ns |
| comment-separated mode | 116.93 ns | 252.78 ns |
| escape-heavy identifier | 112.27 ns | 337.27 ns |
| large CSS | 457.43 µs | 827.54 µs |

`tokenizer/legacy_scanner` measured 472.05 µs on the large CSS fixture, which
provides a direct scanner-only comparison. The focused cases are defined in
[`benches/main.rs`](benches/main.rs).

Validation for this change:

```text
cargo test                         # 20 unit + 3 integration + 281 fixture tests
cargo clippy --all-targets -- -D warnings
```
