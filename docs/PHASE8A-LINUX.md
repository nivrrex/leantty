# PHASE8A-LINUX：流式选区（0.2.1）与吞吐诊断

## S0 取证（已验证事实）

- 选区模型：`Selection { anchor: CellPos, focus: CellPos }`（`crates/app/src/selection.rs:21-24`）；
  原先 `region()` 返回 **矩形** `(min_col,min_row,max_col,max_row)`，`contains()` 用矩形判定，
  渲染层 `decide.rs` 同样按矩形反色 → **是块选（矩形），不是流式**。
- 鼠标处理：左键按下 `start_selection()`（`lib.rs:2825`）——`click_count` 1/2/3 分别建单格/选词/选行；
  移动 `on_cursor_moved` 在 `mouse_down` 时扩展 focus；松开 `commit_selection()`（`lib.rs:2907`）复制到
  CLIPBOARD 与 PRIMARY。
- **零长度选区**：单击（count==1）建 `Selection::new(abs_pos)`（anchor==focus）→ 矩形退化为一个格子 →
  渲染成**一个反色格**。用户看到的"黑框"即此零长度选区（不是光标：光标绘制走 `decide` 的 cursor 分支，
  为白/黑块且位置随输入移动）。
- 已有能力：**双击选词**（`word_region`，按 `semantic_escape_chars` 切词）、**三击选行**（anchor 0..cols-1）
  都已实现，无需新增。

## S1 对拍（alacritty vs leantty，` :99`，xdotool 注入）

| 场景 | alacritty | leantty（修复前） | leantty（修复后） |
|---|---|---|---|
| 单击不拖动 | 无可见标记、清除既有选区 | **留下单格反色块** | 无标记、清除选区 ✓ |
| 同行向右/向左拖选 | 流式（同行即区间） | 同（矩形等价） | 同 |
| 跨 3 行向下拖 | 中间行**整行** | 只取相同列区间 | 中间行整行 ✓ |
| 跨 3 行向上拖 | 中间行整行 | 同上 | 中间行整行 ✓ |
| 拖到行尾之后 | 取到行尾，复制时去掉行尾空白 | 同上 | 同上 |
| 选中后再单击 | 清除选区、不复制 | 复制单格（PRIMARY 被覆盖） | 不复制（PRIMARY 不变）✓ |

alacritty 无块选修饰键需求（本机未观察到）；未新增块选模式。

## S2/S3 实现与验证（已验证事实）

- `selection.rs`：新增 `stream_row_range(anchor, focus, row, cols)`（方向感知：向下拖首行 anchor.col..行尾、
  末行 0..focus.col；向上对称；中间行整行）与 `extract_text_stream(rows, spans, cols)`
  （软折行处不插换行，与既有 `extract_text` 同规则；行尾空白由 `extract_rows` 处理）。
- `lib.rs`：渲染用 `vis_region` 改为 `(anchor_row, anchor_col, focus_row, focus_col)` 并把越界行 clamp 到视口；
  `commit_selection` 按流式逐行取列范围；单击（count==1）不提交、无可见标记。
- `render/src/decide.rs`：`Ctx.selection` 语义改为流式四元组，`in_sel` 用方向感知的行内区间判定。
- 单测：`selection::stream_tests` 3 例（三行向下、三行向上、单行/选区外）。
  **回退为矩形实现时 2 例变红**（`assertion left == right failed`），恢复后转绿。
- e2e：`case76-stream-selection`（三行拖动 → PRIMARY 含中间整行 `bbbbbbbbbb`）、
  `case77-single-click-no-mark`（先选中再单击 → PRIMARY 内容不变）。完整 e2e **130/130**。
- 黄金哈希 `frame_bytes_identical` 仍 **507485fb7945fb8f**；workspace **212 测试通过**。

## S4 版本与文档（已验证事实）

`workspace.package.version` → **0.2.1**；`CHANGELOG.md` 新增 0.2.1（Changed：流式选区、单击不留标记；
Fixed：跨短行拖选丢中间文本）；README 两份在"已知限制"前补一段选区说明；`build-deb.sh`/`export.sh`
仍从 `Cargo.toml` 读版本（无硬编码）。
