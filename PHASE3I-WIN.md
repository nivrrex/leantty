# PHASE3I-WIN：阶段 4I Windows 复验（不稳定量化 / OSC 端到端 / 粘贴取消 / 关窗耗时 / 字体成本）

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4i-done`；量化共享测试不稳定；OSC 4/104 端到端复测；Ctrl-C 取消粘贴复测；窗口关闭耗时；字体加载成本配置对照。未改共用代码、未加依赖、未改 trait。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。测试期间本机另有用户自己的 WezTerm 与安全软件常驻（未结束任何不属于本任务的进程）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/tags/phase4i-done` | `68ec3bf28a101ae8a6a4b7d4c3d40d21408ce611` |
| `refs/tags/phase4i-done^{}` | **无此行** —— 轻量 tag（`git cat-file -t` = commit） |

- 合并：`Merge made by the 'ort' strategy`，提交 **bb26ee0**，0 冲突。
- Windows 专属文件 hash 未变：`crates/pty/src/windows.rs` e99aef81、`crates/pty/tests/pty_windows.rs` c3bb4b20、`docs/WIN-NOTES.md` 6d748725、`.cargo/config.toml` 311a5385；合并 diff 中 `scripts/windows/`、`vt-verify-win/`、`crates/pty/` 前缀 0 文件。

## 2. t2 构建、测试与不稳定量化

| 项 | 结果 |
|---|---|
| debug / release 构建 | rc=0，12.3 s / 8.8 s，**release 零告警** |
| clippy dev / release `--all-targets -D warnings` | 均 rc=0 |
| cargo test --workspace | **rc=101：40 个目标中仅 `app` 的 `writer::tests::paste_queue_respects_cap` 失败**（其余全绿） |
| cargo tree（`-e normal`，windows-gnu） | wgpu 命中 **0**、pollster 命中 **0**；134 行依赖 |

### 2.1 不稳定量化（30 次连续运行）

命令：`cargo test -p app writer::`，连续 30 次。

| 用例 | 失败次数 / 30 | 失败消息原文 |
|---|---|---|
| `writer::tests::paste_queue_respects_cap` | **5 / 30（约 17%）** | `thread 'writer::tests::paste_queue_respects_cap' panicked at crates\app\src\writer.rs:322:9: 接受量不应远超上限（accepted=1114112）`（另一次 accepted=1179648） |
| 其余 `writer::` 用例（8 个） | 0 / 30 | — |

`drain_to_writer` 相关（4I 修过）：`cargo test -p app drain_to_writer` 连续 45 次（30 + 15 次追加），**1 次失败**（未能捕获到具体用例名，失败率约 2%），其余全过。

- 判定：`paste_queue_respects_cap` 是**竞态型共享测试**（断言依赖"写线程尚未消费"），在快机器上必然偶发；`drain_to_writer` 基本稳定。两者均未改，写入请求 1。

## 3. t3 OSC 4 / OSC 104 端到端（release，`inverse-dark.toml`，normal.red = #d32f2f）

避开反色块（ConPTY 会改写 `ESC[7m`），改用**背景色块**：`ESC[41m` + 10 空格 + `ESC[0m`。

序列由 `.ps1` 文件产生（`pwsh -File`），脚本**第一行写标记文件**，标记文件存在 = 注入确实生效。

| 条件 | 序列 | 注入生效 | `#AABBCC` 像素 | 配置红 `#d32f2f` 像素 |
|---|---|---|---|---|
| 系统 conhost | OSC 4 → `ESC[41m` 块 | **是**（marker4.txt） | **1280** | 0 |
| 系统 conhost | OSC 4 → OSC 104 → `ESC[41m` 块 | **是**（marker104.txt） | **1280** | 0 |
| WezTerm conpty | OSC 4 → `ESC[41m` 块 | **是** | **1280** | 0 |
| WezTerm conpty | OSC 4 → OSC 104 → `ESC[41m` 块 | **是** | **1280** | 0 |

- 结论（只写数据）：**OSC 4 在 Windows app 层确实生效**——两种 ConPTY 下色块都变成 `#AABBCC`（10 格 = 1280 px，远超 ≥500 的口径），配置红一个像素都没有。3F-b 的"未核实"由此**转为已验证事实（生效）**；当时失败是注入未生效。
- **OSC 104 未复位**：两种 ConPTY 下执行 OSC 104 后色块仍是 `#AABBCC`、配置红 0 px，与 3G 的字节取证（OSC 104 被 ConPTY 丢弃）一致。

## 4. t4 Ctrl-C 取消粘贴（release，系统 conhost，5 MB × 5）

方法：`ps-win.toml`（powershell.exe shell），真实路径粘贴 5 MB，粘贴中注入 Ctrl-C，随后输入 `echo ok > <标记文件>` 并回车，检查 15 s 内是否出现标记。

| 结果 | 说明 |
|---|---|
| **0 / 5 通过** | 标记文件均未在 15 s 内出现 |
| 追加验证 | 单次 debug 运行等 **90 s** 仍未出现；改用 100 KB 粘贴同样 **0/2** |
| **注入有效性对照** | 不做粘贴、直接输入同样的标记命令：**263 ms 内出现**（证明注入路径与标记机制本身有效） |
| 屏幕证据（debug dump） | 中断后屏幕仍是 `PS C:\Users\Admin> aaaa…`，整屏被粘贴文本占满 |

- 已验证事实：**Windows 上 Ctrl-C 之后终端不会在可接受时间内回到可用提示符**——粘贴字节已经进入控制台输入缓冲/PSReadLine 行内，Ctrl-C 之后这批文本仍要被逐字消费；随后键入的命令会拼在已存在的 `aaa…` 之后，因而标记命令不会作为独立命令执行。
- 与 Linux 的差别：Linux 上是"readline 等括号粘贴结束标记"导致卡住；Windows 的机制不同（ConPTY 已吞掉标记），但**结果不是 5/5**。
- app 侧的"丢弃未写出数据"仍然有效（3H 证据：5 MB 粘贴中 Ctrl-C 后子进程只收到 123 130 字节）。本次的 0/5 是"已送达数据无法撤回"，不是队列没丢。写入请求 2。

## 5. t5 窗口关闭耗时（release，各 5 次）

| 场景 | 退出耗时（ms） | stderr 关键行 |
|---|---|---|
| 无粘贴 | **464 / 460 / 423 / 432 / 390** | 无异常行 |
| 粘贴 5 MB 进行中（前台 `ping -t` 不读 stdin） | **2448 / 2447 / 2461 / 2467 / 2438** | **`pty writer: join 超时（2s），已分离线程（对端长期不读）`**（5/5 均出现，原文如上） |

- 退出后检查：`Get-Process ping` = 0、`Get-Process app` = 0（只按名字统计自己关心的两类；未结束任何属于用户的进程）。
- 结论：无粘贴时关闭 <0.5 s；粘贴中关闭固定约 2.45 s，等于写线程 join 的 2 s 上限 + 收尾，与日志一致。只记录，不改代码。

## 6. t6 字体加载成本配置对照（release，各 5 次）

配置：A = `examples/windows.toml`（Consolas + Microsoft YaHei 回退）；B = Consolas、无 fallback；C = Courier New、无 fallback。三组只有字体项不同。

| 组 | fontdb 中位/最小 | cpu_groups 中位/最小 | **first_frame 中位 / 最小** | 每面 font_ms 中位（Regular/Bold/Italic/BoldItalic） |
|---|---|---|---|---|
| A（Consolas + YaHei 回退） | 41.6 / 36.8 | 878.5 / 791.0 | **1440 / 1378** | 155 / 137 / 133 / 133（两次噪声运行 213–647） |
| B（Consolas 无回退） | 41.0 / 39.3 | 785.9 / 770.4 | **1336 / 1245** | 143 / 139 / 137 / 127 |
| C（Courier New 无回退） | 40.9 / 37.8 | 853.5 / 841.7 | **1376 / 1365** | 169 / 166 / 130 / 133 |

- 三组原始值：A 3216/4788/1378/1440/1397；B 1352/1336/1599/2312/1245；C 1365/1376/1472/1403/1554。
- 只写数据：**A 比 B 的中位高 104 ms、最小值高 133 ms**，两者唯一的差别是 A 配了 Microsoft YaHei 回退；**C 与 B 的差别更小**（中位 +40 ms、最小 +120 ms）。fontdb 三组都在 37–42 ms，与本机噪声同量级。
- 工程判断（数据直接支持的部分）：回退字体的配置在首帧上表现出 100 ms 量级的代价，但样本只有 5 次且本机噪声大（最大与最小差 1–2 s），**不足以把差异归因到具体阶段**，故不作因果结论。

## 7. 未解决问题

1. `paste_queue_respects_cap` 17% 的失败率（共享测试竞态，请求 1）。
2. Ctrl-C 无法让 Windows 终端在 15 s 内回到可用提示符（输入缓冲/PSReadLine 里已送达的 5 MB 文本仍被消费），与任务预期 5/5 不符（请求 2）。
3. 粘贴中关窗固定耗时 2.45 s = 写线程 join 上限 2 s；是否可缩短属设计取舍，只记录。
4. 字体对照组噪声大（同组内首帧差 1–3 倍），三组差异只能给量级，不能给因果。

## 8. 对共享代码的请求

1. **请求 1（测试稳定性）**：`crates/app/src/writer.rs::tests::paste_queue_respects_cap` 在 Windows 上 30 次失败 5 次（17%），失败消息：`接受量不应远超上限（accepted=1114112）`。断言依赖写线程尚未消费的时序；建议用可控阻塞 sink，或只断言 `queued_bytes() <= WRITE_QUEUE_CAP`。
2. **请求 2（行为/文档）**：Ctrl-C 取消粘贴在 Windows 上的实际表现是"队列丢弃生效、已送达数据无法撤回"，因此终端在 15 s（实测 90 s 仍）不可用。若产品上希望"Ctrl-C 后立即可用"，需要与 shell/ConPTY 交互层面的方案（例如粘贴前先清空控制台输入缓冲，或粘贴期间不分批直写），属共用逻辑，本任务未改。
3. （信息）OSC 4 已证在 Windows app 层生效，3F-b 请求 19 的"未核实"可关闭；OSC 104 被 ConPTY 丢弃已在 3G 登记。

## 9. 交付物与证据

- 脚本：`vt-verify-win/{run_3i_osc,run_3i_ctrlc,run_3i_ctrlc_ctl,run_3i_close,run_3i_font}.ps1`；序列发生器 `vt-verify-win/osc/{osc4_bg,osc104_bg}.ps1`（各含写标记文件）。
- 配置：`scripts/windows/e2e/cfg/{t6b-consolas-only,t6c-courier-only}.toml`。
- 日志：`vt-verify-win/logs/{3i-clippy-dev,3i-clippy-rel,3i-test,3i-tree,3i-writer-30,3i-drain-30}.log`；`vt-verify-win/logs/phase2b-win/{3i-sys-os4,3i-wez-os4,3i-*,t6-*}.{png,err}`；标记文件在 `vt-verify-win/osc/`。
- 未入库：`perfdata/`、任何 conpty.dll/OpenConsole.exe。

## 10. 手动清单（≤3）

1. 在空闲机器上重跑 §6 的三组对照（本机噪声导致同组内首帧差 1–3 倍）。
2. 用真实使用场景确认"Ctrl-C 后仍要等粘贴文本被消费完"是否可接受（§4）。
3. 若关注关窗 2.45 s，可实测缩短写线程 join 上限的取舍（§5，本次未改代码）。
