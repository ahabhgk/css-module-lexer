## 执行计划：先保证 TokenStream 严格单向

本阶段只处理 TokenStream 和重复扫描，不改 CI/bench 配置、不动 SmallVec、不继续扩大 generic value 快进范围，也不增加 Visitor 兼容层。

目标执行流：

```text
Lexer.scan_pos
  只由 next_token / raw fast-forward 向前推进
                ↓
TokenStream lookahead buffer
  peek 只追加 token，不回退 scanner
                ↓
TokenStream consumed_end
  next 只消费已有 token，始终向前
                ↓
Dependency handlers
  inspect → commit，不再 set_position 重解析
```

核心不变量：

```text
previous_scan_pos <= scan_pos
previous_consumed_end <= consumed_end <= scan_pos
每个源码区间最多 tokenize 一次
每个 token 最多 dispatch 一次
```

### 1. 建立当前基线

先保存当前 HEAD 的结果和性能：

- 对所有 fixture、所有 `Mode` 保存完整 dependency/warning 输出。
- 本地运行默认 benchmark。
- 本地运行 tokenizer/dependency sink microbench。
- 额外记录 bootstrap、Rspack fixture 的 token 数、冒号数和 scanner 前进字节数。

命令：

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo bench --bench main -- --noplot
CSS_MODULE_LEXER_RUN_MICRO_BENCHES=1 cargo bench --bench main -- --noplot
```

这些结果作为本阶段 differential correctness 和 Criterion baseline。

### 2. 重构 TokenStream 游标模型

修改 [lexer.rs](/Users/bytedance/rstack/css-module-lexer/src/lexer.rs:1043)。

将位置职责拆清楚：

- `Lexer::scan_pos`：已经完成 tokenization 的最远位置，只能前进。
- `TokenStream::consumed_end`：dependency scanner 已消费的最远位置，只能前进。
- lookahead buffer：保存已扫描但尚未消费的 token。

`TokenStream::next()` 调整为：

```text
buffer 非空 → pop_front → 更新 consumed_end
buffer 为空 → lexer.next_token → 更新 consumed_end
```

消费缓存 token 时禁止调用 `Lexer::set_position`，避免把 `scan_pos` 拉回 token end。

保留现有 `VecDeque`，本阶段不做容器替换，避免同时引入额外变量。

### 3. 删除所有 rewind API

删除或替换：

- `TokenStream::rewind_to_current`
- `TokenStream::clear_peeked_at`
- 可向后移动的 `TokenStream::set_position`
- `TokenStream::set_lexer_position`
- `Lexer::semantic_pos`
- `Lexer::sync_scan_position`

新的 API 只允许单向操作：

```rust
peek()
peek_n()
peek_non_comment()
next()
consume_if()
fast_forward_if_buffer_empty()
```

在 debug/test 构建中增加单调性断言，一旦 scanner 或 consumed position 后退立即失败。

### 4. 先重写冒号和点号 lookahead

修改 [dependencies.rs](/Users/bytedance/rstack/css-module-lexer/src/dependencies.rs:1501)，这是收益最高的路径。

冒号按 context 分流：

```text
DeclarationName
  colon 直接进入 Value context
  不做任何 mode/selector lookahead

Selector / TopLevel
  peek 第一个 token
       ↓
  普通 pseudo：只需要一个 token
       ↓
  只有 ident == local/global
  才继续 peek_non_comment 检查第二个 token
```

具体要求：

- `color:red` 不再读取 `red` 后面的 `;`。
- `red` 即使被 peek，也直接从 buffer 消费，不重新扫描。
- `:local/**/.foo` 的 comment 和 `.foo` 全部留在 buffer 中，按源码顺序消费一次。
- 识别 pseudo 后只消费对应 token，不清空后继 buffer。
- 点号 class lookahead同样只缓存和消费，禁止调整 lexer position。

### 5. 改造特殊 handler，使用 inspect → commit

逐个处理当前调用 `stream.set_position(...)` 的函数，优先顺序：

1. mode pseudo、class、id
2. local var/function declaration
3. composes
4. ICSS import/export
5. `@value`
6. keyframes/container/counter-style 等 at-rule

改造模式：

```text
旧：
next token
  ↓
发现不匹配
  ↓
set_position(token.start)
  ↓
主循环重新扫描

新：
peek token
  ↓
匹配成功才 next/commit
  ↓
不匹配则 token 保留在 buffer
  ↓
主循环直接消费已有 token
```

已经消费的 token 不允许重新塞回或重新 tokenize。helper 需要首个 token 时，将 token 作为参数传入，不再根据 range 重建 cursor。

### 6. 让 fast-forward 与 lookahead 协作

本阶段暂不扩大 [fast_forward_generic_value](/Users/bytedance/rstack/css-module-lexer/src/lexer.rs:279) 的扫描能力，只修正调用条件：

- lookahead buffer 非空时禁止 raw fast-forward。
- 先正常消费所有 buffered token。
- buffer 为空后，才允许从当前 `scan_pos` 向前快进。
- fast-forward 结束后不得修改 `consumed_end`，下一个正式 token 再完成语义消费。

这样先确保快进不会被缓存 token 覆盖或把 scanner 拉回去。

### 7. 增加针对单向性的测试

新增最小输入测试：

```css
.a { color: red; }
.a { width: 10px; }
.a { color: red; background: blue; }
:local(.a) {}
:local/**/.a {}
:global/**/#id {}
.a { color: var(--color); }
.a { background: url("./a.png"); }
.a { color: rgb(1, 2, 3); }
```

测试内容：

- dependency/warning 与当前基线完全一致。
- token range 严格递增。
- `scan_pos` 从不后退。
- `consumed_end` 从不后退。
- `color:red` 中 `red` 只被 tokenize 一次。
- Pure magic comment 只处理一次。
- comment-separated mode 仍保持 dependency 和 warning 顺序。
- malformed CSS 不依赖回退才能恢复。

### 8. 本地验收

正确性硬门槛：

- 所有 Rust test、integration/fixture test 通过。
- clippy 无 warning。
- 所有 Mode 的 dependency 和 warning 内容、范围、顺序完全一致。
- 测试期间没有任何位置回退。
- `rg` 不再找到 `rewind_to_current`、`sync_scan_position` 或向后 `set_position`。

性能硬门槛：

- tokenizer microbench 不退化超过 2%。
- 默认四个 benchmark 不允许出现显著退化。
- `dependency_sink/large_css` 必须改善。

阶段目标：

- bootstrap 本地改善至少 8%。
- Rspack global/local 改善至少 5%。
- `lex_streaming` self time 明显下降。
- `fast_forward_generic_value` 不再扫描后又被缓存 token 覆盖。

如果严格单向完成后仍未达到目标，先重新采集本地 profile，再进入下一阶段的 property classifier 和 fused generic-value scanner 优化。CI 只用于后续观察，不修改其配置。
