# Lexer 性能优化记录与执行计划

## 最新 CodSpeed 基线

PR [ahabhgk/css-module-lexer#8](https://github.com/ahabhgk/css-module-lexer/pull/8)
的最新 CodSpeed 报告更新于 2026-08-05，对应 head `c191601`、base `a752289`：

| Benchmark     |   Base |   Head | CodSpeed 结论              |
|---------------|-------:|-------:|----------------------------|
| bootstrap     | 6.9 ms | 6.1 ms | 改善 13.17%                |
| rspack global |    N/A | 1.7 ms | 新 benchmark，无 main 基线 |
| rspack local  |    N/A | 2.0 ms | 新 benchmark，无 main 基线 |
| rspack pure   |    N/A | 2.0 ms | 新 benchmark，无 main 基线 |

当前能由 CodSpeed 直接证明的跨分支改善只有 bootstrap。Rspack 三项用于观察 PR
连续提交之间的趋势；在 main 出现相同 benchmark 前，不能宣称相对 main 的改善比例。
每轮还要核对 comparison SHA 和 runner；runner 变化时，simulation time 只记录，不作为
阶段验收证据。

CodSpeed 分支页：
[intellild:codex/refactor-u8-lexer](https://app.codspeed.io/ahabhgk/css-module-lexer/branches/intellild%3Acodex%2Frefactor-u8-lexer)

## 最终本地 profile 热点

2026-08-05 在同一台机器上使用 macOS `sample` 采集最终未提交工作区：bootstrap 6819 个
sample，Rspack Local 6773 个 sample，并补采 Rspack Global 1788 个、Pure 5080 个 sample。
表中展示样本更充足的 bootstrap/Local top-of-stack self sample，占比按各自总 sample 计算；
多个 GenericValue scanner monomorph 已合并统计。Global/Pure 的热点排序一致，未出现新的
allocation 或 layout 热点。

| Hot path                                      | bootstrap | Rspack Local |
|-----------------------------------------------|----------:|-------------:|
| identifier scan                              |     31.4% |        15.8% |
| `LexDependencies::lex_streaming`              |     14.6% |        12.0% |
| `Lexer::next_token`                           |      8.8% |        18.4% |
| Generic/AtRule/SpecialValue raw scanner       |      9.0% |         6.6% |
| `handle_ident`                                |      4.6% |         2.9% |
| selector scanner + TokenStream wrapper        |      2.8% |         2.1% |
| significant-token peek                        |      2.6% |         3.1% |
| `DependencyContext::push_dependency`          |      0.5% |         0.4% |
| `memmove`                                     |      0.3% |         0.9% |
| HashMap reserve/rehash                        |       N/A |         0.4% |
| `SmallVec::extend`                            |       N/A |         0.1% |

与 `c191601` 的阶段 1 profile 相比，bootstrap 的 `next_token` self sample 从约 14.0%
降到 8.8%；Rspack Local 从约 24.2% 降到 18.4%。最终 allocation、Context push、SmallVec
和 `memmove` 均未形成值得继续改布局的热点，因此阶段 7 明确跳过。

## 范围与硬约束

- `TokenStream` 必须严格单向。`scan_pos` 和 `consumed_end` 只能前进，已消费的源码区间
  不得重新 tokenize。
- 只有 lookahead buffer 为空时才能进入 raw fast-forward；buffer 非空时先按源码顺序
  消费已有 token。
- 正确性目标是 `DependencyContext` 中 dependency 的类型、内容、range 和顺序正确。
  不为旧 visitor 回调时机或无关 warning 顺序增加兼容代码。
- warning 只保留当前仍有产品语义的测试，例如 Pure mode 校验；不把旧 visitor 的 warning
  顺序当成性能优化的兼容门槛。
- 不修改 CI、CodSpeed 配置、benchmark 源码或 fixture。本地可以运行现有 benchmark，
  CI/CodSpeed 只用于观测提交后的变化。
- 每个提交只包含一个性能变量及其必要测试。存储布局、scanner、classifier 和容量系数
  不混在同一个提交中。
- 所有 raw scanner 必须复用现有 string、comment、escape 和 balanced 语义，不维护第二套
  CSS tokenizer。

## 目标执行流

```text
TokenStream
  lookahead buffer 非空
       └─ next 按源码顺序消费已有 token

  lookahead buffer 为空
       ↓
  根据 ScanContext 选择单向 raw scanner
       ├─ Selector：跳到 class/id/mode 或 selector 边界
       ├─ DeclarationName：扫描一次并完成 property 分类
       ├─ AtRule：dependency at-rule 精确解析，其余 dependency-aware skip
       ├─ GenericValue：复用现有 dependency-aware scanner
       └─ SpecialValue：只在当前 property 关心的候选处停止
       ↓
  在停止位置恢复 tokenization
       ↓
LexDependencies
  inspect → commit
       ↓
DependencyContext
  直接写入已确认的 dependency 和 side-table payload
```

核心不变量：

```text
previous_scan_pos <= scan_pos
previous_consumed_end <= consumed_end <= scan_pos
已消费的源码区间不会再次 tokenization
raw scanner 不跨过 dependency candidate 或语义边界
```

## 已完成基线

| Commit    | 已完成内容                                                 |
|-----------|------------------------------------------------------------|
| `2b698f5` | TokenStream 严格单向                                       |
| `72fd718` | declaration colon 不做 selector/pseudo lookahead           |
| `632c65a` | GenericValue dependency-aware fast-forward 与 lookahead 收敛 |
| `4c01d4b` | Dependency 变长 payload 外置到 DependencyContext            |
| `c191601` | scanner 直接写 Context、容量估算、移除中转容器、FxHashSet   |

### DependencyContext 存储

`Dependency` 已从 160 bytes 降为 80 bytes。变长数据分别存入：

- `Vec<Dependency>`
- composes local classes
- composes names
- `@value` import items

`Composes` 和内部 `ValueAtRule::Import` 只保存带类型参数的半开索引 range，避免不同
side table 的 range 误用。

主 dependency Vec 在扫描前按源码长度和 Mode reserve。稀有 side table 只保存估计值，
第一次写入时才 reserve，避免普通 CSS 为不存在的 composes 或 `@value` payload 分配。

| Fixture/mode    | dependency length | dependency capacity |
|-----------------|------------------:|--------------------:|
| bootstrap local |              6177 |                7275 |
| rspack local    |              1853 |                1940 |
| rspack global   |               694 |                 776 |
| rspack pure     |              1853 |                1940 |

Rspack side table 的估计与实际值：

| Mode                | local classes | names | `@value` items |
|---------------------|--------------:|------:|---------------:|
| local/pure estimate |            97 |   107 |             34 |
| local/pure actual   |            82 |    99 |             31 |
| global estimate     |            16 |    16 |             35 |
| global actual       |            11 |    11 |             31 |

### 已移除和保留的中间容器

已经直接写入 Context 或改成 iterator/streaming build：

- grid template area ranges
- unescaped composes names 的二次收集
- `@value` import items
- ICSS export comment ranges

当前保留的 `SmallVec` 都用于尚未确认的语法状态：

- `BalancedStack`：嵌套配平和 mode scope
- `ImportPrelude`：完成 `@import` prelude 后才能判定节点角色
- selector local classes：完成 selector list 后才能判定 composes 是否合法
- composes item：需要识别 `from`、混合 `global()`、dependency 顺序和错误恢复
- `@value` tokens：需要在完整 rule 上识别 nesting、`from`、comma、colon 和 `as`

ICSS symbol 集合使用 `rustc_hash::FxHashSet`。该集合不暴露迭代顺序，因此替换 hasher
不改变 dependency 结果。

## 阶段总览

| 阶段 | 优先级 | 状态     | 独立变量                         | 主要性能信号                   |
|------|--------|----------|----------------------------------|--------------------------------|
| 0    | —      | 已完成   | 单向流、GenericValue、Context    | 当前实现基线                   |
| 1    | P0     | 已完成   | 当前 HEAD baseline/profile       | 得到可信的新热点排序           |
| 2    | P0     | 已完成   | Selector raw fast-forward        | `next_token`/selector dispatch  |
| 3    | P0     | 已完成   | AtRule dependency-aware raw skip | at-rule prelude token 数        |
| 4    | P0     | 已完成   | SpecialValue fast-forward        | special property token 数       |
| 5    | P1     | 已完成   | ASCII identifier 快路径          | identifier scan self time       |
| 6    | P1     | 已完成   | property/at-rule/ICSS 分类        | visitor self time/字符串比较    |
| 7    | P2     | 已跳过   | 剩余 allocation/layout           | 无可测 allocation 热点          |

实际按 1 → 2 → 3 → 4 → 5 → 6 → 7 执行。当前实现仍在工作区，尚未提交；因此本文只记录
阶段边界和本地证据，不填写虚构的 commit SHA。若后续提交，应按 scanner、identifier、
classifier 拆分，不把 CI 或 benchmark 文件带入提交。

## 详细执行计划

### 阶段 1：重新建立当前 HEAD baseline 与 profile

1. 在同一台机器、相同 release profile 下运行默认四组 benchmark 和已有 opt-in micro
   benchmark，记录 commit SHA、Criterion interval 和 outlier 数量。
2. 使用与旧热点表相同的 profiler 重新采集 bootstrap、Rspack Local、Global、Pure，
   分别记录 inclusive/self time。旧表不再作为阶段 2 之后的定量依据。
3. 使用临时本地 instrumentation 记录：
   - `next_token` 次数
   - raw fast-forward 字节数
   - lookahead buffer 命中数
   - 各 ScanContext dispatch 次数
   - dependency/side-table Vec growth 次数
4. instrumentation 不提交，不修改 bench 源码或 fixture；完成记录后删除临时代码。
5. 对 bootstrap、Rspack fixture 的所有 Mode 记录 dependency 数量以及
   type/content/range/order checksum，用于回归侦测。checksum 只是辅助证据，新增 fast path
   仍必须有显式期望测试，不能盲目固化当前行为。

交付物：新的热点表、baseline SHA、本地 benchmark 记录和 context/token 计数。如果新
profile 不再支持当前排序，先更新“阶段总览”，再开始代码优化。

执行结果：baseline SHA 为 `c191601`。首次绝对中位数为 bootstrap 623.93 µs、Rspack
Local 202.90 µs、Global 183.34 µs、Pure 203.31 µs；因机器频率波动，阶段比较改用交错运行
的 clean clone。五组 fixture 的 dependency checksum、warning 数和容量数据已记录，临时
profiler/instrumentation 未提交并已删除。

### 阶段 2：Selector raw fast-forward

涉及文件：`src/lexer.rs`、`src/dependencies.rs`、`tests/test.rs`、
`tests/integration_test.rs`。

1. 在 `src/lexer.rs` 增加 selector 专用的单向 raw scanner，只允许 TokenStream 在 buffer
   为空时调用。
2. 普通 element、whitespace、combinator 和不相关 pseudo name 直接跳过。
3. Attribute selector、string 和普通 comment 作为 opaque range 向前扫描，不能把内部
   的 `.`, `#`, `:` 或 braces 误当 dependency candidate。
4. 遇到以下内容时停止并恢复 tokenization：
   - class、id
   - 任意可能是 `:local`、`:global`、`:import`、`:export` 的 colon
   - selector list、nesting、block 和 malformed 边界
   - Pure magic comment
5. 普通 pseudo 只跳过名称；`:is()`、`:not()`、`:has()` 等函数体仍按 selector nesting
   扫描，不能整段作为 opaque function 跳过。
6. 仅在 `ScanContext::Selector` 启用，不改变 DeclarationName、AtRule 或 value 路径。

新增测试至少覆盖：escaped class/id、comment-separated mode、attribute selector、string、
nested selector、`:is()`/`:not()`/`:has()`、selector list、Pure comment、未闭合 attribute/
function 和其他 malformed selector。

验收信号：Local/Global/Pure dependency 的 type/content/range/order 不变；selector 路径的
`next_token` 和 dispatch 次数下降；单向 cursor 测试继续通过。

执行结果：增加 `fast_forward_selector`，attribute 使用跨调用 square depth 保持 opaque，
Pure magic comment、函数、字符串和结构边界交还 tokenizer。小于 256 bytes 的输入不启用
该 scanner；大输入再用 4-byte probe 避免只跨过一个空格/combinator 的无效调用。新增
selector candidate、attribute、nested pseudo、composes 合法性测试；单向 cursor 与全部
fixture dependency checksum 通过。

### 阶段 3：AtRule dependency-aware raw skip

涉及文件：`src/lexer.rs`、`src/dependencies.rs`、`tests/test.rs`。

1. 在 `handle_at_keyword` 中先分类 at-rule。
2. 以下 dependency 相关 at-rule 继续精确解析：
   - `@import`、`@value`、`@charset`、`@namespace`
   - keyframes 及 vendor prefix
   - container、property、counter-style、font-palette-values、function
   - `@scope`
3. 已知不产生 dependency 的普通 prelude 使用 balanced raw scanner 前进到顶层 `{` 或
   `;`，正确跳过 string、comment、escape 和嵌套括号。
4. 未知/custom at-rule 不直接整段跳过，先走保守的 dependency-aware scanner；遇到
   `url()`、`image-set()`、`selector()`、mode pseudo、Pure magic comment 或 malformed
   边界时恢复 tokenization。
5. 只有显式测试证明某类 prelude 不会产生 dependency 后，才能加入完全快速分类。

新增测试至少覆盖：media、supports、layer、document、`@supports selector(.class)`、
URL/image-set、嵌套函数、字符串内 `{`/`;`、custom at-rule、Pure comment 和未闭合 prelude。

验收信号：普通 prelude 不再逐 token dispatch；进入 block 后的 ScanContext、scope 和
nesting level 与当前语义一致；所有 prelude 内合法 dependency 保留。

执行结果：复用 GenericValue raw scanner，增加“顶层 `{` 停止”模式和独立 AtRule nesting
state；`@import`/非法 import 仍走精确路径。新增 nested function、string 中 `;`/`{`、URL、
var、ICSS symbol 和 block boundary 测试。Rspack dependency checksum 不变；减少的 warning
来自把 at-rule prelude 误当 selector 的旧 `NotPure` 假阳性。

### 阶段 4：SpecialValue dependency-aware fast-forward

涉及文件：`src/lexer.rs`、`src/dependencies.rs`、`tests/test.rs`。

| PropertyKind | 必须保留的 property-specific 候选                       |
|--------------|---------------------------------------------------------|
| Animation    | 顶层非 reserved name、逗号、timing/duration 分界         |
| ListStyle    | 顶层非 reserved counter-style name、position/image 分界 |
| FontPalette  | `--` palette name                                       |
| Container    | 顶层非 reserved container name、slash                   |
| Grid         | 按 GridPropertyKind 保留 area/name、string、slash        |

所有 SpecialValue 还必须保留通用 URL、`var()`、local custom function、ICSS symbol、
delimiter 和 malformed 边界，不能只处理表中的 property-specific 候选。

1. 为五种 PropertyKind 分别定义最小停止集合，不使用覆盖所有 property 的宽泛集合。
2. 数字、dimension、whitespace、普通函数和确定不相关的 identifier raw 跳过。
3. 候选 name、字符串、逗号、slash、括号边界和 dependency function 处恢复 tokenization。
4. 复用 GenericValue 的 balanced state 和 string/comment/escape 处理，不复制 scanner。

新增测试至少覆盖：每种 property 的 shorthand、多个 name、多层函数、CSS-wide keyword、
escaped name、custom property、URL/var/ICSS symbol、大小写变体和 malformed value。

验收信号：特殊 property 的 declaration/reference dependency 完全一致；普通 declaration
value 不得误启动 property parser；目标 property 的 token 数下降。

执行结果：Animation 保留顶层 ident/comma，ListStyle/Container/Grid 仅保留非 reserved
候选，FontPalette 仅保留 `--`，Grid 额外保留 string；通用 dependency function、ICSS 和
Pure comment 继续优先停止。Rspack Local/Pure 从 1853 项变为 1850 项，唯一差异是删除
custom-property `{ ... }` token block 中错误产生的 `.class`、颜色 hash 和 `#id` selector
dependency；已用显式测试固定正确行为。

### 阶段 5：ASCII identifier 快路径

涉及文件：`src/lexer.rs` 及 lexer 单测。

1. 在 `scan_name_with_flags` 前增加 plain ASCII 扫描层，连续处理 name byte，直接返回
   ASCII flags。
2. 遇到反斜杠、NUL 或 non-ASCII 时进入现有完整慢路径。慢路径保持唯一，避免两套 CSS
   escape 语义漂移。
3. GenericValue/Selector raw scanner 只请求实际需要的结果，例如 end position、是否
   function 和是否可能是 dependency candidate，不为普通 name 完整生成 TokenFlags。
4. 不使用 unchecked chunk load 跨过 input end；任何向量化或 word scan 都必须先证明
   边界安全和 UTF-8 boundary 正确。

新增测试至少覆盖：plain ASCII、1–6 位 hex escape、escaped newline、NUL replacement、
non-ASCII、invalid slice boundary 防护、identifier/function 边界和 escape-heavy fixture。

验收信号：`scan_name_with_flags` self time 下降；plain ASCII tokenizer micro 改善；
escape-heavy micro 无可重复显著回退。

执行结果：`scan_name` 与 `scan_name_with_flags` 共享一个 const-generic 核心；raw scanner
编译掉 TokenFlags 更新，tokenizer 保留同一 escape/NUL/non-ASCII 慢路。escape-heavy
tokenizer 中位数约 85.1 ns（阶段前约 90.7 ns），dependency sink 约 364.9 ns（基本持平）。

### 阶段 6：property、at-rule 与 ICSS 分类收敛

涉及文件：`src/dependencies.rs`、`tests/test.rs`。

1. 合并 `classify_property` 和 `grid_property_kind`，一次返回 `PropertyKind` 与可选
   `GridPropertyKind`，避免同一 declaration name 重复比较。
2. property 不处于 local property mode 时只检查 `composes`/`compose-with`，不运行
   custom-property、animation、list-style、container、grid 等 local classifier。
3. 将 `handle_at_keyword` 的重复字符串比较收敛为一次 `AtRuleKind` 分类。
4. property 和 at-rule 都先按长度、首字节、vendor prefix 或 `--` 前缀分派，再做必要
   的 ASCII case-insensitive 比较。
5. ICSS symbol lookup 只在可能产生 `ICSSSymbol` 的 value context 执行；保留现有 length
   filter、bit filter 和 `FxHashSet`，不在 Selector/BlockItem 查询。

新增测试至少覆盖：vendor-prefixed animation/keyframes、大小写变体、相似但无关的
property/at-rule、CSS/Local/Global/Pure mode 分流，以及 ICSS symbol 出现在 selector、
property name、GenericValue 和 SpecialValue 的场景。

验收信号：visitor self time 和字符串比较次数下降；不增加 lookahead、兼容状态或新的
hash lookup。

执行结果：property classifier 一次返回 `PropertyKind` 和可选 `GridPropertyKind`；非
property-local mode 只分类 composes。at-rule 先按长度分类为 `AtRuleKind`，后续不再重复
比较字符串。ICSS lookup 仅在 GenericValue、SpecialValue 和 block 内 AtRule prelude 执行，
Selector/BlockItem/DeclarationName 不查表；新增显式 context 测试。

### 阶段 7：按新 profile 处理剩余 allocation（条件执行）

涉及文件取决于 profile；没有可测热点时明确跳过。

1. 重新采集四个主 benchmark 的 allocation、`memcpy` 和 Vec growth 数据。没有可测热点
   时结束，不为了删除类型而改写 parser。
2. 如果 composes item 或 `@value` token buffer 成为热点，先设计 Context checkpoint/
   rollback API，再评估直接写入 side table。必须保持 typed range 连续、混合 `global()`
   dependency 顺序和错误恢复；无法证明时保留 `SmallVec`。
3. `ImportPrelude`、selector local classes 和 `BalancedStack` 属于未确认语法状态，不直接
   写结果 Context。只有 profile 证明其分配显著时，才单独调整 inline capacity。
4. 只有 fixture corpus 出现实际 realloc 时才重新校准 `DependencyContextCapacity`，
   不因单个样本扩大经验容量。

该阶段不得把 SmallVec payload 放回 `Dependency` enum，也不得把 parser 临时状态暴露
为公开 API。

执行结果：跳过。最终 Rspack Local profile 中 Context push 约 0.4%、HashMap reserve/rehash
约 0.4%、`SmallVec::extend` 约 0.1%、malloc/free 合计低于 0.5%；没有证据支持继续修改
inline capacity、checkpoint/rollback 或存储布局。

## 最终本地结果

默认 benchmark 使用临时 clean clone 的 `c191601` 与当前工作区交错运行。数值受机器频率
影响，只用相邻运行判断方向；Criterion 目录中跨时段的自动 `change` 百分比不作为证据。

| Benchmark     | `c191601` clean | 当前工作区 | 方向       |
|---------------|----------------:|-----------:|------------|
| bootstrap     |       631.23 µs |  631.79 µs | 基本不变   |
| Rspack Local  |       209.40 µs |  204.75 µs | 改善约 2.2% |
| Rspack Global |       183.03 µs |  168.53 µs | 改善约 7.9% |
| Rspack Pure   |       215.31 µs |  208.57 µs | 改善约 3.1% |

关键 micro 的最终绝对中位数：escape-heavy tokenizer 85.14 ns、dependency sink 364.93 ns；
large CSS tokenizer 274.51 µs、dependency sink 624.93 µs。阶段前对应值分别约 90.73 ns、
365.23 ns、279.00 µs、663.94 µs。短 nested-selector 输入低于启用阈值，不进入 Selector raw
scanner；最后一次 sink interval 为 435–446 ns，Criterion 判定与同工作区前次无显著变化。

最终 fixture 记录：

| Fixture/mode    | dependencies | warnings | checksum           |
|-----------------|-------------:|---------:|--------------------|
| bootstrap local |         6177 |        0 | `c83664d667e02f9e` |
| rspack css      |           17 |        3 | `8bee453186382fba` |
| rspack local    |         1850 |       47 | `2d4ca05b8a84676c` |
| rspack global   |          694 |       78 | `bd926d31f88aa7b7` |
| rspack pure     |         1850 |      136 | `2d4ca05b8a84676c` |

Local/Pure 的 dependency 变化仅为前述 3 个 custom-property 假 selector；Global/CSS/bootstrap
结果不变。warning 数减少来自 at-rule prelude 和 custom-property token block 的旧 selector
假阳性，不恢复 visitor/spacing 兼容逻辑。

## 每阶段验证和提交门槛

```text
实现前
  cargo test --all
       ↓
实现后正确性与静态检查
  cargo fmt --all -- --check
  cargo test --all
  cargo clippy --all-targets --all-features -- -D warnings
       ↓
本地性能
  cargo bench --bench main -- --noplot
  CSS_MODULE_LEXER_RUN_MICRO_BENCHES=1 cargo bench --bench main -- --noplot
       ↓
检查 diff 范围并创建独立提交
       ↓
推送后观察 CodSpeed bootstrap + rspack local/global/pure
       ↓
更新本文的 SHA、状态和数据
```

| 证据                   | 硬门槛                                                                              |
|------------------------|-------------------------------------------------------------------------------------|
| dependency correctness | 新 fast path 有显式期望测试；type/content/range/order 正确                          |
| 单向性                 | cursor 单调性测试通过；不存在 rewind 或已消费区间重扫                               |
| 全量回归               | `cargo test --all` 通过                                                             |
| 静态检查               | fmt 通过；clippy 全 targets/features 且 `-D warnings` 通过                          |
| 本地性能               | 默认四组无可重复显著回退；目标 micro/token count 出现方向一致的改善                 |
| 远端性能               | 记录 CodSpeed head/base SHA、runner；Rspack 无 main 基线时只比较 PR 连续提交         |
| 变更范围               | diff 不包含 CI、bench 源码、fixture 或 visitor 兼容层                               |

本地 Criterion 结果互相矛盾或 outlier 过多时，在相同电源和温度条件下重复三次，比较
interval，不使用最快单次结果。CodSpeed 只有在 runner 一致且 comparison SHA 正确时才作为
阶段证据；否则回到本地 profile 判断。

出现以下任一情况时，不进入下一阶段：

- dependency type/content/range/order 错误
- TokenStream 游标回退或重复扫描
- 默认 benchmark 出现可重复显著劣化
- 目标 token/dispatch/lookup 指标没有下降，且代码复杂度增加
- 实现依赖 visitor 兼容分支或修改 CI/bench/fixture

阶段性能不达标时保留正确性测试，撤回该优化实现，重新 profile 后再决定下一步。

## 整体完成定义

整个优化计划只有同时满足以下条件才算完成：

1. 当前工作区 profile 已替换旧热点表；每个执行或跳过的阶段都有独立 diff、数据和理由。
   用户要求提交时，再按阶段拆分 commit。
2. 阶段 2–6 均已独立执行，或新 profile 证明对应目标不再是热点并明确跳过。
3. 最终重新采集 bootstrap 和 Rspack Local/Global/Pure；`next_token`、name scanning、
   visitor dispatch、lookahead、allocation 中不存在一个仍可通过本文方案获得明确收益
   却未处理的热点。
4. 阶段 7 已执行，或新 profile 明确证明没有值得处理的 allocation/layout 热点；保留的
   `SmallVec` 均有语法状态、连续 range、dependency 顺序或错误恢复理由。
5. dependency correctness、单向性、全量 Rust tests、fmt 和 clippy 硬门槛全部通过。
6. 本地完成不依赖推送；代码提交并推送后，再记录新的 CodSpeed comparison，并核对
   head/base SHA 和 runner。
7. 本文的实现、热点、阶段状态、最终数据和剩余非目标项与代码一致。

## 暂不处理

- 旧 profile 中 `VecDeque::grow` 约为 0.02%。阶段 1 会重新确认，但在出现新证据前不替换
  TokenStream 容器。
- 不修改 CI、CodSpeed 配置、benchmark 源码或 fixture。
- 不恢复旧 Visitor API，也不为 visitor 回调时机或无关 warning 顺序增加兼容代码。
- 不在没有 profile 证据时继续缩小 enum、扩大预分配或删除承担 parser state 的容器。
