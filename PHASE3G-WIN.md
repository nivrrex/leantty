# PHASE3G-WIN：阶段 4G Windows 复验（视口上滚隐藏光标 + ConPTY 字节取证）

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4g-done`；在真实 Windows 上复验"视口上滚隐藏光标"；抓 ConPTY 输出原始字节，判定 3F-b 的两个现象是否为平台行为。未改共用代码、未加依赖、未改 Pty/Renderer trait。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/tags/phase4g-done` | `afb2444c1e40b2f117a50aa16566213c4f55bb5a` |
| `refs/tags/phase4g-done^{}` | **无此行** —— 轻量 tag（`git cat-file -t` = commit） |
| `refs/heads/main`（同时实测） | `3bcb1f770782d3ff92cff34b76a8e5d50d4275af` |

- Linux 报告称 main=3bcb1f7、tag=afb2444，**实测两者确实不同**（tag 指向 afb2444，main 已前进到 3bcb1f7）。
- 合并：`Merge made by the 'ort' strategy`，提交 **a595584**，0 冲突（win 侧与 4G 分叉，非快进）。
- Windows 专属文件 hash 未变：`crates/pty/src/windows.rs` 96416daa、`crates/pty/tests/pty_windows.rs` c3bb4b20、`docs/WIN-NOTES.md` 141f8d8c、`.cargo/config.toml` 311a5385；合并 diff 中 `scripts/windows/`、`vt-verify-win/`、`crates/pty/` 前缀 0 文件。

## 2. t2 构建与测试

| 项 | 结果 |
|---|---|
| debug 构建 | rc=0，13.4 s |
| release 构建 | rc=0，11.8 s，**零告警** |
| clippy dev / release `--all-targets -D warnings` | 均 rc=0（logs/3g-clippy-dev.log、3g-clippy-rel.log） |
| cargo test --workspace | rc=0，**39 个测试目标全绿，0 FAILED** |
| `cargo test -p vt --test cursor_viewport` | **4 passed**（hidden_when_viewport_scrolled_above_cursor、position_follows_viewport_offset_when_still_visible、hidden_on_alternate_screen、hidden_mode_wins_over_viewport） |
| `cargo test -p render --test cursor_viewport_pixels` | **1 passed**（cursor_pixels_absent_after_scroll_up_and_back_after_bottom，3.8 s） |
| cargo tree（`--target x86_64-pc-windows-gnu -e normal`） | wgpu 命中 **0**、pollster 命中 **0**；134 行依赖 |

## 3. t3 视口上滚隐藏光标（release app，真实 Windows）

配置 `scripts/windows/e2e/cfg/cursor-magenta.toml`：背景 #000000、文字 #e0e0e0、**光标 #ff00ff（品红）**，与文字/反色都不同色。
计数口径：先把除目标色（±40/通道）外的像素全部视为背景，再对目标色像素计数（等价"非目标色变黑、目标色变白再数白"）。

| 阶段 | 光标色像素 | 说明 |
|---|---|---|
| a) 输出 200 行后（底部） | **128** | 恰好一个 8×16 单元格 |
| b) 滚轮上滚 5 格后第 1 次 | **0** | |
| b) 第 2 次（+0.6 s） | **0** | |
| b) 第 3 次（+0.6 s） | **0** | 三次跨越光标闪烁周期，仍为 0 |
| c) 滚轮回到底部第 1/2/3 次 | **128 / 128 / 128** | 光标恢复 |
| c) 回底画面 vs a) 画面 | **diffPixels = 0** | 视口确实回到同一位置 |

- 已验证事实：视口上滚时游标被隐藏（≥1 s 后连续 3 次采样均为 0），回底后恢复且画面与初始逐像素一致。
- 期间修正一处**测试工具**缺陷（非产品问题）：`mouse_event` 的 `dwData` 传 `0xFFFFFF88` 时 PowerShell 把它解析成 Int32 −120，绑定到 `uint` 参数抛 `MethodException`，导致"滚轮下滚"从未真正发出（早前 2B c2a 的"下滚"同样静默无效）。改为 `[uint32]4294967176` 后 c) 阶段通过。

## 4. t4 ConPTY 原始字节取证

方法：用已有的原始读端探针 `probe_ptyin.exe`（直接 `ReadFile` 读 ConPTY 主端，**不经过本 app**），分别以系统 conhost 与 WezTerm conpty（conpty.dll + OpenConsole.exe sideload 到探针同目录）运行；每组序列用两种产生方式（`pwsh -File` 与原生 `bash printf`）。原始十六进制转储存于 `vt-verify-win/logs/conpty-seq/`。

### 4.1 到达字节（解码后）

| 序列 | 系统 conhost | WezTerm conpty |
|---|---|---|
| **s1** `ESC[7m` + 10 空格 + `ESC[0m` | `ESC[2J` **`ESC[30m` `ESC[47m`** `ESC[H` `ESC[10X` `ESC[10C` … `ESC[m` | `ESC[?9001h ESC[?1004h ESC[?25l ESC[2J ESC[m` **`ESC[7m`** `ESC[H` `'          '` … |
| **s2** `ESC[37m ESC[7m` + 10 空格 | **`ESC[30m` `ESC[47m`** + `ESC[10X/10C`（显式白被反色吞掉） | **`ESC[37m` `ESC[7m`** 原样 + 10 个空格 |
| **s3** `ESC[31m` + `REDTEXT` | `ESC[31m` ✔（bash 组另带 `ESC[49m`） | `ESC[31m` ✔ |
| **s4** `OSC 4;1;rgb:aa/bb/cc` + `ESC[31m` + `OSC 104` | **`OSC 4` 到达 ✔；`OSC 104` 缺失 ✘** | **`OSC 4` 到达 ✔；`OSC 104` 缺失 ✘** |
| **s5** `OSC 0;T3GTITLE` | 到达 ✔ | 到达 ✔（另带 WezTerm 自身的 `ESC[?9001h/1004h/25l` 启动序列） |

- 两种产生方式（`pwsh` 与原生 `bash printf`）结论一致，只是 conhost 会在开头插入自己的 `ESC[2J ESC[m ESC[H` 与标题 `OSC 0`；不影响被判定序列。
- 系统 conhost 会把 10 个空格**改写为 `ESC[10X` / `ESC[10C`**（擦除+光标前移），WezTerm conpty 保留原始空格。

### 4.2 逐项判定

1. **`ESC[7m`（默认色反色）**：系统 conhost **改写成显式颜色 SGR** `ESC[30m`（黑前景）+ `ESC[47m`（白背景）；WezTerm conpty 原样保留 `ESC[7m`。
   → **请求 18 属 ConPTY 平台行为**：app 收到的是显式 palette 索引，黑色是索引 0、白底是索引 7，因此反色块显示为配置的 `normal.white`（本机 #757575），与 `primary.foreground` 无关。这与 3F-b 的屏幕实测 (#757575) **完全吻合**。
2. **`ESC[31m` 与 `ESC[37m`**：原样到达（`ESC[31m` 未被改写成 RGB），app 用配置 palette 解析，因此 3F-b 的"红 = #d32f2f"正确。
3. **`OSC 4`**：**两种 ConPTY 都原样送达**。
   → 请求 19 **不能归因于 ConPTY**：OSC 4 到达了 app，3F-b 里"OSC 4 不生效"的 app 层观察因此**未核实**（当次三次尝试的输入注入本身就没把文字打出来，证据不足）。
4. **`OSC 104`（重置调色板）**：**两种 ConPTY 都丢弃**（到达字节里完全没有 `ESC]104`）。
   → 这是平台行为：在 Windows 上"OSC 4 改色后无法通过 OSC 104 复位"。
5. **`OSC 0` 标题**：两种条件都到达 ✔（与 3E 观察到的标题可变一致）。

### 4.3 结论

- (18) 反色底色为 `normal.white`：**ConPTY 平台行为**（conhost 把 `ESC[7m` 物化为 `ESC[30m/ESC[47m`），不是 app 缺陷。WezTerm conpty 保留 `ESC[7m`，若 app 在那种条件下运行才会走自身反色逻辑。
- (19) OSC 4：**不是** ConPTY 平台行为（序列到达）；OSC 104 才是（被吞）。3F-b 的 app 层否定结论作废，改写为**未核实**。

## 5. 未解决问题

1. 3F-b 的"OSC 4 在 app 层不生效"缺乏有效证据（输入注入失败导致截图无文字）；本轮证明 OSC 4 能到达 app，需一次可靠的端到端复测（建议在 Linux 侧或用稳定的注入方式）。
2. `OSC 104` 在 Windows 上被两种 ConPTY 吞掉——这是平台限制，需在文档/配置建议中体现（改色后无法复位）。
3. t3 修正的 `mouse_event` `dwData` 传参问题影响此前若干"下滚"用例（2B c2a 等）：那些用例当时未断言下滚方向，结论不受影响，但相关脚本已按需修正。

## 6. 对共享代码的请求

1. **请求 21（文档/行为说明，非缺陷）**：Windows 上 ConPTY 会把 `ESC[7m` 物化为 `ESC[30m`+`ESC[47m`（见 §4.1），因此"默认色反色块 = normal.white"是平台表现；建议在 DECISIONS 或渲染说明里记录该差异，避免后续把它当 bug 追。若要跨平台一致，可考虑 renderer 侧对"inverse 已由 VT 物化"的识别（需改共用逻辑，本任务不做）。
2. **请求 22（平台限制登记）**：`OSC 104` 在 Windows 两种 ConPTY 下均不到达，`OSC 4` 改色后无法复位；建议在配置/文档中标注该限制。
3. （3F-b 请求 19 需降级）OSC 4 已证到达，原"app 不生效"证据不足，建议 Linux 侧用端到端用例复核后再定级。

## 7. 交付物与证据

- 脚本：`vt-verify-win/run_3g_cursor.ps1`（光标可见性，含颜色掩码计数）、`run_3g_conptybytes.ps1`（字节取证驱动）、`vt-verify-win/conpty-seq/*.ps1`（5 组序列的 pwsh 发生器）。
- 配置：`scripts/windows/e2e/cfg/cursor-magenta.toml`（品红光标）。
- 证据：`vt-verify-win/logs/{3g-clippy-dev,3g-clippy-rel,3g-test,3g-tree}.log`；`vt-verify-win/logs/conpty-seq/{system,wezterm}-s{1..5}-{pwsh,bash}.txt`（原始十六进制）；`vt-verify-win/logs/phase2b-win/t3-*.png`。
- 未入库：`probe_inv/target/`、`perfdata/`、任何 conpty.dll/OpenConsole.exe。

## 7.5 推送与打 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `32fb9ee2712269dfbae762abac3c8cf77a0b323f` |
| `refs/tags/phase3g-win-done`（tag 对象） | `52dad280fa3f885498869b25284ffe17be13dcdd` |
| `refs/tags/phase3g-win-done^{}`（解引用 commit） | `32fb9ee2712269dfbae762abac3c8cf77a0b323f` |

- `git push origin win`：`62a8d5a..32fb9ee  win -> win`；tag 为新建后 push（`* [new tag] phase3g-win-done`）。全程未使用任何 force，未移动任何已有 tag。

## 8. 手动清单（≤3）

1. 在 Linux 上用同一份 `cursor-magenta.toml` 复核"上滚隐藏光标"，确认跨平台一致。
2. 用 WezTerm 自身终端跑一次 `OSC 4` 改色 + `ESC[31m`，确认 OSC 104 的缺失是否同样存在（本机只测了 ConPTY 主端字节）。
3. 若后续要把"反色 + 默认色"做成跨平台一致，先在 Linux 上确认 conhost 不做物化时 app 的表现（对应请求 21）。
