# Lexer 性能优化记录

## 最新 CodSpeed 结果

PR [ahabhgk/css-module-lexer#8](https://github.com/ahabhgk/css-module-lexer/pull/8)
最新完成的 CodSpeed run 为 `6a72b7540e00702835f24fd8`，对应提交
`632c65a`：

| Benchmark | Simulation time |
| --- | ---: |
| bootstrap | 6.40 ms |
| rspack global | 1.80 ms |
| rspack local | 2.10 ms |
| rspack pure | 2.10 ms |

相对上一轮 PR run，CodSpeed 显示四项改善约 18%–22%，但 runner 从 Intel
Xeon 6973P 更换为 AMD EPYC 9V74。Simulation 的 cache model 会随 runner
变化，因此不能把全部差值归因于代码。相对 `main`，只有 bootstrap 存在共同
benchmark，CodSpeed 判定为无显著变化；Rspack 三项在 `main` 上尚无对应基线。

## 当前热点

| Hot path | bootstrap | rspack local/pure |
| --- | ---: | ---: |
| `Lexer::next_token` inclusive | 37.3% | 37.4% |
| `Lexer::scan_name_with_flags` self | 21.1% | 12.6% |
| `LexDependencies::lex_streaming` self | 14.5% | 11.3% |
| dependency output `memcpy` | 8.8% | 8.3% |
| `peek_significant_skipping_comments` inclusive | 12.3% | 13.3% |
| generic-value fast-forward inclusive | 11.6% | 6.3% |

总时间中约 89%–93% 是 instruction execution，cache miss 约为 1%。后续优化
应优先减少 token 数量、visitor dispatch 和字符串判断，而不是调整 cache
layout。

## 后续执行流

```text
ScanContext
  Selector
    raw 跳过普通 element、whitespace、combinator、普通 pseudo
         ↓
    只在 class / id / local-global mode / selector 边界停下

  DeclarationName
    扫描一次并完成 property 分类
         ↓
    colon 后直接进入 value scanner

  GenericValue
    保留现有 dependency-aware raw scanner

  AtRule
    先识别 dependency 相关 at-rule
         ├─ import/value/keyframes/container/property/...：精确解析
         └─ 普通 media/supports/...：balanced raw skip 到 `{` 或 `;`
```

## 优化优先级

### P0：扩展 context-aware fast-forward

- Selector 中跳过 dependency 不关心的普通语法，只保留 `.class`、`#id`、
  `:local`、`:global`、嵌套边界和 Pure magic comment。
- 普通 at-rule prelude 在识别关键字后 raw skip；只有 dependency 相关 at-rule
  进入精确 parser。
- SpecialValue 中跳过数字、空白和不影响 dependency 的普通函数，仅在候选
  identifier、字符串或分隔符处恢复 tokenization。

这是最有可能同时降低 `next_token`、`lex_streaming` 和 lookahead 成本的阶段。

### P1：ASCII identifier 快路径

将 `scan_name_with_flags` 拆成两级：

```text
plain ASCII name
  chunk/table scan，直接返回 ASCII flags
       ↓ 遇到
escape / NUL / non-ASCII
       ↓
现有完整 CSS escape scanner
```

GenericValue raw scanner 对普通 ident 只需要 name end、function 标记和 dependency
候选判断，不必为每个普通名称完整生成 TokenFlags。

### P1：收敛 property 和 ICSS 分类

- ICSS symbol lookup 只在 GenericValue/SpecialValue context 执行，不在 Selector
  或 BlockItem 查询。
- property 不处于 local mode 时只检测 `composes`，不扫描 animation、list-style、
  container、grid 等特殊属性。
- property classifier 一次返回 `PropertyKind` 和 `GridPropertyKind`，避免重复调用
  `grid_property_kind`。
- property/at-rule 先按长度和首字节分派，再进行 case-insensitive 比较。

### P2：缩小 Dependency 存储

数据结构调整前 `Dependency` 大小为 160 bytes，Local/Pure flamegraph 中约 8% 的时间用于把
dependency 写入结果 Vec。现有源码长度估算已经避免 realloc：

| Fixture/mode | length | capacity |
| --- | ---: | ---: |
| bootstrap local | 6177 | 7275 |
| rspack local | 1853 | 1940 |
| rspack global | 694 | 968 |
| rspack pure | 1853 | 1940 |

因此进一步增加 `with_capacity` 没有收益。应把 `Composes` 等少见 variant 的变长
payload 外置到 context，enum 只保存索引 range，降低所有 dependency 的 move/copy
成本。

本次数据结构调整后，`Dependency` 从 160 bytes 降为 80 bytes。变长数据分别存入
`DependencyContext` 的 dependencies、composes local classes、composes names 和
`@value` import items 四个 Vec，`Composes` 与内部 `ValueAtRule::Import` 只保留 8-byte
的半开索引 range。该阶段不改变扫描策略，后续性能变化可以单独归因于存储布局。

## 暂不处理

- `VecDeque::grow` 在最新 flamegraph 中只有约 0.02%，不值得预分配或替换容器。
- 不修改 CI、CodSpeed 配置或 benchmark fixture。
- 每次只落一个优化变量，本地验证 dependency/warning 的内容、范围和顺序后，
  再通过 CodSpeed 观察变化。
