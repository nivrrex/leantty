# leantty 0.2.14

新增"拖入文件插入路径"，并修复双击选词在含宽字符（CJK/emoji）行上的错位。

## 新增：拖拽文件到窗口

- 拖入的每个文件把路径插入终端输入，末尾带一个空格（可连续拖多个）。
- 引用规则与 alacritty 一致：只含安全字符的路径原样插入；含空格/元字符时
  Unix 用单引号包裹（内部的 `'` 写成 `'\''`），Windows 用双引号包裹。
- 插入走与剪贴板粘贴同一条路径（积压上限、回底、主动排帧），对端开了 `?2004` 时作为一次粘贴块处理。
- 代码：`crates/app/src/paste.rs::quote_dropped_path`、`AppHandler::on_dropped_file`、
  `WindowEvent::DroppedFile`。

## 修复：双击选词在宽字符行上错位

缺陷成因：`word_region` 把**格子列**当成**字符下标**去查一行的字符串。纯 ASCII 行两者相等，
含宽字符的行就不等，于是：

| 操作 | 修复前 | 修复后 |
|---|---|---|
| 双击 `中文测试` 里的 `文` | 选到 `中文`（应为整段连写的 `中文测试`） | `中文测试` |
| 双击 CJK 之后的 `word` 里的 `r` | `ord`（少了一个字符） | `word` |

修法：新增 `selection::word_region_cells`，直接在格子上走——跳过宽字符的续格（`SpacerTail`）、
以"该格文本首字符"判定 `semantic_escape_chars` 分隔符、点在宽字符续格时归位到头格，
返回头格列范围。高亮由 `decide()` 的 `span=2` 自动覆盖续格、文本提取跳过续格，两者因此严格一致
（这消除了"最后一个字符被复制但没反色 / 右键粘贴多一个字符"这一类 off-by-one）。

## 测试

- 单测：`word_region_cells_handles_wide_chars`（宽字符整段、续格归位、宽字符之后英文精确、
  分隔符只选自身、空行不 panic）、`word_region_cells_matches_char_region_on_ascii`（ASCII 行为不变）、
  `dropped_paths_are_quoted_per_platform`（安全字符/空格/单引号/`>&`/Windows 反斜杠与空格）。
- e2e 新增两例（合计 134 项，全绿）：
  - `case82-drop-file-path`：`LEANTTY_TEST_DROP` 钩子（与 `DroppedFile` 同一方法）插入
    `/…/case82-my file.txt`，dump 首行必须出现 `'…my file.txt'`；
  - `case83-word-select-wide`：真实双击 CJK 第 2 字 → 剪贴板必须是 `中文测试`；
    `case83-paste-matches-selection` → 右键粘贴后必须存在"提示符+选中文本"结尾的行，
    且不存在多一个字符的行。

## 验证

- `cargo nextest run --all`：252 passed / 1 skipped。
- `cargo clippy --all-targets -D warnings`：0 告警；release 构建 0 告警。
- `scripts/e2e/run_all.sh`：**PASS=134 FAIL=0**，GPU 轮 10/10。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.14_amd64.deb` | 3,522,376 B | `ab061bccaf995bee2b6ca6b577cd6524d7e008be378a5292e4951a1322661c53` |
| `leantty-0.2.14-windows-x86_64.zip` | 5,546,790 B | `39be5247a30db61e291c87a828011b63245ac8ab31a3e55aa660dc2665043b13` |

## 未验证

- 真实 XDND 拖放（Xvfb 下无法可靠模拟一次真正的拖拽）：本次验证走的是 debug 钩子，它与
  `WindowEvent::DroppedFile` 调用同一个方法，但没有验证 winit 的拖放事件本身在 Windows/macOS
  上的投递；Windows 侧需要实机拖一次确认。
- 多文件一次性拖入（winit 逐个投递，逐个追加空格）未实测。
