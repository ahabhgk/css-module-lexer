# SpecialValue Fast-Forward 优化记录

## 结论

本轮以 commit `823432d` 为 clean baseline，只保留 Animation 的上下文分流：

```text
ScanContext::SpecialValue(property)
       ↓
property == Animation
       ├─ 是：不进入 raw scanner
       │      继续通过 TokenStream 单向 tokenization
       └─ 否：进入现有 dependency-aware raw scanner
              保留 property candidate、ICSS、URL、var、
              local function、comment 和 balanced 边界
```

没有引入策略 enum、策略对象、trait dispatch 或 vtable。分流是
`fast_forward_special_value` 外层的一个稳定 `if`；Animation 之外仍复用同一套 string、comment、escape、number 和 balanced
scanner。

同时删除了只供 Animation 使用的 `preserve_top_level_comma` runtime 参数。Animation 已经不进入 raw scanner，剩余
PropertyKind 不需要在顶层逗号前停止。

## PropertyKind A/B 决策

每次实验只关闭一个 PropertyKind 的 raw scan，不修改 CI、benchmark 源码或 fixture。

| PropertyKind | 最终决策      | 本地 A/B 结论                                            |
|--------------|---------------|----------------------------------------------------------|
| Animation    | 关闭 raw scan | 正反向 A/B 中 Local/Global/Pure 一致改善，bootstrap 持平 |
| Grid         | 保留 raw scan | bootstrap/Global 出现回退，Local/Pure 没有稳定净收益     |
| ListStyle    | 保留 raw scan | bootstrap、Local、Global 分别约回退 2.0%、2.9%、5.1%     |
| Container    | 保留 raw scan | 四组基本持平，Local/Global 略慢，没有可归因收益          |
| FontPalette  | 保留 raw scan | `--` candidate 稀疏，没有数据支持关闭                    |

Animation 的相邻正反向 A/B：

| 操作                    | bootstrap | Rspack Local | Rspack Global | Rspack Pure |
|-------------------------|-----------|--------------|---------------|-------------|
| 关闭优化、恢复 raw scan | 持平      | +4.35%       | +2.81%        | +2.68%      |
| 再启用 Animation 分流   | 持平      | 改善约 3.3%  | -2.65%        | -3.09%      |

百分比为 Criterion 相邻 change 或相邻绝对中位数；机器频率有波动，因此不使用最快单次结果。 第二行 Local 的 Criterion
interval 较宽，但绝对中位数从 207.28 µs 降到 200.49 µs， 且反向实验方向一致。

最终候选与临时 clean `823432d` 副本的同机结果：

| Benchmark     | clean `823432d` |  最终候选 | 观察   |
|---------------|----------------:|----------:|--------|
| bootstrap     |       628.09 µs | 611.19 µs | 无回退 |
| Rspack Local  |       203.89 µs | 194.30 µs | 改善   |
| Rspack Global |       169.76 µs | 165.52 µs | 改善   |
| Rspack Pure   |       202.84 µs | 202.54 µs | 持平   |

这两轮之间的 bootstrap 也有整体频率变化，所以表格只用于最终无回退检查；Animation 的 可归因结论以前面的相邻正反向 A/B 为准。

## Profile 复核

使用 Criterion `--profile-time` 和 macOS `sample` 分别采集 Rspack Local/Pure。 样本是 top-of-stack self sample，占比按各报告总
sample 归一化。

| Hot path                         | Local baseline | Local final | Pure baseline | Pure final |
|----------------------------------|---------------:|------------:|--------------:|-----------:|
| `Lexer::next_token`              |         17.76% |      17.80% |        16.84% |     19.33% |
| identifier scan                  |         16.90% |      15.67% |        16.32% |     16.54% |
| `LexDependencies::lex_streaming` |         11.84% |      10.72% |        12.75% |     10.51% |
| raw value scanner monomorphs     |          5.12% |       6.34% |         5.53% |      5.89% |
| raw scanner + `lex_streaming`    |         16.96% |      17.06% |        18.28% |     16.40% |

Animation 回到 tokenizer 后，Pure 的 `next_token` 占比上升符合预期；Local 的
`next_token` 基本不变。aggregate raw scanner 同时包含 GenericValue、AtRule 和其他 SpecialValue，不能把其比例变化直接解释为
Animation scanner 成本。源代码分流保证 Animation raw scanner 调用为零，最终 Criterion 证明工作搬移没有造成总耗时回退。

曾实验为 ListStyle、FontPalette、Container、Grid 生成四套独立 callback。它在 Local 样本中降低了 raw scanner，但 Pure 中 raw
scanner 占比反而上升，且增加代码体积，因此 最终撤回。保留一个 Animation `if` 与一个共享 `FnMut` 路径。

## 正确性与单向性

`TokenStream` 的约束保持不变：

```text
previous_scan_pos <= scan_pos
previous_consumed_pos <= consumed_pos <= scan_pos
buffer 非空时 raw fast-forward 直接返回
已消费源码不会 rewind 或重新 tokenize
```

现有测试覆盖：

- TokenStream cursor 单调前进、peek 后消费 buffer 不重扫。
- Animation name、shorthand、多个逗号项、timing function、duration、vendor prefix、 escaped name、CSS-wide keyword、`var()`、
  `env()` 和 ICSS symbol。
- Grid quoted area/line name/slash，ListStyle、FontPalette、Container dependency。
- URL、local function、Pure comment、comment-separated mode 和 malformed input。

使用临时校验程序对完整 `DependencyContext`（dependency 与全部 side table）执行确定性 顺序 hash，最终结果与 clean `823432d`
完全一致；临时程序未保留：

| Fixture/mode    | dependencies | warnings | checksum           |
|-----------------|-------------:|---------:|--------------------|
| bootstrap local |         6177 |        0 | `f1dbec03f663c1fc` |
| rspack css      |           17 |        3 | `3111445646046f82` |
| rspack local    |         1850 |       47 | `841fad14d9dd4544` |
| rspack global   |          694 |       78 | `7dc4ad54fddc25b5` |
| rspack pure     |         1850 |      136 | `841fad14d9dd4544` |

checksum 用于证明 dependency 类型、内容、range 和顺序没有变化；显式行为测试仍是主要 正确性证据。

## 验证结果

- `cargo fmt --all -- --check`
- `cargo test --all`：18 个库测试、10 个 integration tests、283 个行为测试全部通过
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo bench --bench main -- --noplot`
- Local/Pure Criterion profiling 与 macOS sample
- JetBrains CLion inspection：`src/dependencies.rs` 无错误
- `git diff --check`

## 变更范围

最终代码只修改：

- `src/dependencies.rs`：Animation 上下文分流
- `src/lexer.rs`：删除不再需要的顶层逗号 runtime option
- 本文：记录最终决策和验证证据

没有修改 CI、CodSpeed 配置、benchmark、fixture、DependencyContext、SmallVec、HashMap 或 visitor compatibility。代码尚未提交或推送。
