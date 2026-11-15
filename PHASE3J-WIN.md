# PHASE3J-WIN：阶段 4L Windows 复验（稳定性 / 输入回底 / 默认键 / 粘贴拒绝 / 字体面成本）

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4l-done`；写线程测试稳定性复测；真实 Windows 上验证输入回底、默认 Ctrl+Shift+C、粘贴积压拒绝；字体面成本。未改共用代码、未加依赖、未改 trait。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。测试期间用户自己的 WezTerm 与安全软件常驻（未结束任何不属于本任务的进程）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/tags/phase4l-done` | `79588161d4465af8513ad0e15fdab40e4f04c1ec` |
| `refs/tags/phase4l-done^{}` | **无此行** —— 轻量 tag（`git cat-file -t` = commit） |

- 合并方式：**快进**（`9ce7daa` 是 HEAD 祖先；Linux 侧已并入 win 的 3I 工作），HEAD = 7958816，0 冲突。
- Windows 专属文件 hash 未变：`crates/pty/src/windows.rs` e99aef81、`crates/pty/tests/pty_windows.rs` c3bb4b20、`docs/WIN-NOTES.md` 0f90261c、`.cargo/config.toml` 311a5385。

## 2. t2 构建、测试与稳定性

| 项 | 结果 |
|---|---|
| debug / release 构建 | rc=0，15.5 s / 15.3 s，**release 零告警** |
| clippy dev / release `-D warnings` | 均 rc=0 |
| cargo test --workspace | **rc=0，40 个目标全绿**（4J 修复后不再有失败） |
| cargo tree（`-e normal`） | wgpu 命中 **0**、pollster 命中 **0** |

稳定性（本机，连续运行）：

| 目标 | 次数 | 失败 | 说明 |
|---|---|---|---|
| `cargo test -p app writer::` | 30 | **0** | 4J 修复前为 **5/30**（17%，`接受量不应远超上限（accepted=1114112）`）；本轮未复现 |
| `cargo test -p app --lib follow` | 10 | **0** | 5 个 follow 用例 |
| `cargo test -p app --lib key_matches` | 10 | **0** | |

## 3. t3 输入回底（release，真实路径）

判据：右侧 12 px 滚动条拇指的底端像素位置（0–479，越接近 479 越靠底）+ dump 中可见的最后行序号。

| 项 | 观测量 | 判定 |
|---|---|---|
| a) 输出 300 行后 | 拇指底 **479/480**，lastLine **300** | 在底部 |
| a) 滚轮上滚 6 格 | 拇指底 **424/480**，lastLine **242** | **离开底部** ✔ |
| b) 键入 `echo MARK > <文件>`（marker 1139 ms 出现 = 注入生效） | 拇指底 **479/480**，lastLine **300** | **输入回到底部** ✔ |
| c) 上滚后再**短粘贴**（不回车） | 拇指底保持 **424/480** | **未回底** ✘（见下） |
| c2) 上滚后**大粘贴**（1.2 MB，走分块路径） | 拇指底 424 → **479/480** | 回到底部 ✔ |
| d) 上滚期间后台输出（`ping -t`，无任何输入） | 拇指底 408 → **408** | **保持不动** ✔ |
| d) 上滚期间执行 Copy（Ctrl+Insert，配置绑定） | 拇指底 408 → **408** | **保持不动** ✔ |

- **c 的差异是共用代码缺口（已验证事实）**：`crates/app/src/lib.rs` 中 ≥1 MB 的分块路径调用了 `follow_viewport_before_input(InputEvent::Paste)`（第 2676 行），而 <1 MB 的 `paste_text_accepted()` 路径**没有调用**，因此短粘贴不会回底。`follow.rs` 的规格注释明确把"粘贴"列为应回底的输入类别。属共用逻辑，未改，写入请求 1。
- 其余三项与规格一致；注入有效性均以标记文件为据。

## 4. t4 默认 Ctrl+Shift+C（配置不含自定义 keyboard.bindings）

配置 `scripts/windows/e2e/cfg/ctrlshift-default.toml`：由 `examples/windows.toml` 去掉全部 `[[mouse.bindings]]`/`[[keyboard.bindings]]`，只保留字体/颜色/滚动等，shell=cmd.exe。

| 轮次 | 输出可识别文本 | 拖选后 Ctrl+Shift+C 的剪贴板 | 随后命令 |
|---|---|---|---|
| 1 | `CSELECTME` | **`CSELECTME`** | `echo MARK > 文件` → 标记文件内容 **`MARK`**，文件名正确 |
| 2 | 同上 | **`CSELECTME`** | 同上 |
| 3 | 同上 | **`CSELECTME`** | 同上 |

- **3/3 通过**：剪贴板等于所选文本；后续命令正常执行且命令行没有被多打进字符 `C`（若 CTRL+SHIFT+C 被当普通字符送进 PTY，命令会变成 `Cecho MARK…`，标记文件不会生成）。4K 之前的问题未复现。

## 5. t5 粘贴积压拒绝（release，系统 conhost）

场景：前台 `ping -t 127.0.0.1`（不读 stdin），连续用真实路径粘贴 5 MB。

| 项 | 数据 |
|---|---|
| 拒绝发生 | **第 3 次粘贴时** |
| 日志原文 | `[perf] paste_rejected bytes=5242880 backlog=7471104 cap=12582912 oversized=false` |
| 是否触发"单条超限"分支 | 否（`oversized=false`，是积压达到上限） |
| RSS | 1518 MB（paste1）→ 1521 MB（paste3 拒绝后）→ 1521 MB（Ctrl-C 后），**有界**（全程 +3 MB） |
| 拒绝后 Ctrl-C + `echo MARK > 文件` | 标记文件 **15 s 内未出现**（与 3I 的"已送达粘贴无法撤回"一致；本项按要求仅记录，不据此判失败） |

- 只记录：拒绝语义为"整条拒绝"（不写任何字节），日志同时给出 backlog 与 cap。

## 6. t6 字体面成本（release，默认配置 Consolas + Microsoft YaHei 回退，5 次运行 × 4 组 = 20 个样本）

| 指标 | 中位 | 最小 |
|---|---|---|
| `font_group_ms` total | **168.4** | **144.7** |
| `font_seg_ms` face_ms | **0.00** | 0.00 |
| `font_seg_ms` read_ms | **0.7** | 0.4 |
| `font_seg_ms` parse_ms | **23.9** | 20.9 |
| `font_cache_ms` | **7.2** | 6.0 |
| **未归属**（total − read − parse − cache） | **137.0** | **112.5** |

段间墙钟（同一批样本）：

| 区间 | 中位 | 最小 |
|---|---|---|
| group_begin → seg 打印 | 73.7 | 63.5 |
| seg → cache 打印 | 59.0 | 51.9 |
| cache → group 结束 | 31.7 | 25.0 |

- **回答"每组约 100 ms 的缺口落在哪"**：**不在 `face_ms`（恒为 0.00）**，也不在 read（0.7 ms）或 parse（23.9 ms）。缺口是 `font_group_ms` 内部、已打印各点之间未被计时的部分：**begin→seg 约 74 ms、seg→cache 约 59 ms、cache→end 约 32 ms**，合计约 137 ms（占每组 168 ms 的约 81%）。
- 首次运行的 4 组为 1764/175/145/199 ms（一次 1.7 s 的噪声），已作为异常值排除在中位数之外（上表用全部 20 个样本的统计，中位不受影响）。
- 只写数据：`face_ms=0` 与墙钟 74 ms 的差异说明该点未覆盖实际工作，具体落在哪一步需要新打点，本任务不下结论。

## 7. 未解决问题

1. 短粘贴（<1 MB）不回底，大粘贴（≥1 MB）回底 —— 共用代码两条路径不一致（请求 1）。
2. 粘贴积压拒绝后 Ctrl-C 仍无法让提示符在 15 s 内可用（与 3I 一致，属"已送达数据无法撤回"）。
3. 字体组内约 137 ms 无打点覆盖，`face_ms` 恒 0，无法从现有数据判断具体步骤（请求 2）。

## 8. 对共享代码的请求

1. **请求 1（行为一致性，已验证）**：`crates/app/src/lib.rs::paste_text_accepted()`（<1 MB 路径）未调用 `follow_viewport_before_input(InputEvent::Paste)`，而 ≥1 MB 分块路径调用了；结果是小粘贴在视口上滚时不回底，与 `follow.rs` 规格（粘贴应回底）不一致。Windows 实测：上滚后拇指底 424，小粘贴后仍 424；1.2 MB 粘贴后回到 479/480。
2. **请求 2（观测增强）**：`font_seg_ms` 的 `face_ms` 恒为 0.00，而 `font_group_ms` 内 begin→seg 与 seg→cache 各有约 74/59 ms 墙钟未计时；建议在这两段内补点（例如 face 查询完成、缓存构建完成），否则"每组约 100 ms"无法归因。

## 9. 交付物与证据

- 脚本：`vt-verify-win/{run_3j_follow,run_3j_follow2,run_3j_ctrlshift,run_3j_reject,run_3j_font}.ps1`。
- 配置：`scripts/windows/e2e/cfg/ctrlshift-default.toml`（无自定义绑定）。
- 日志：`vt-verify-win/logs/{3j-clippy-dev,3j-clippy-rel,3j-test,3j-tree,3j-writer-30,3j-follow-10,3j-keymatches-10}.log`；`vt-verify-win/logs/phase2b-win/{t3*,t4*,t6j-*}.{png,err}`；标记文件在 `vt-verify-win/osc/`。
- 未入库：`perfdata/`、任何 conpty.dll/OpenConsole.exe。

## 9.5 推送与打 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `eb2b8aef88774625e99573ee971392b8b5b507d2` |
| `refs/tags/phase3j-win-done`（tag 对象） | `23c40713862596c5232cd1c432b4206b7e239d12` |
| `refs/tags/phase3j-win-done^{}`（解引用 commit） | `eb2b8aef88774625e99573ee971392b8b5b507d2` |

- `git push origin win`：`9ce7daa..eb2b8ae  win -> win`；tag 为新建后 push（`* [new tag]`）。全程未使用 force，未移动任何已有 tag。

## 10. 手动清单（≤3）

1. 在空闲机器上复测 §6（本机同组内噪声仍有 1.7 s 级异常值）。
2. 用真实使用场景确认"短粘贴不回底、长粘贴回底"的差异是否影响体感（请求 1）。
3. 若关注启动，可在字体组内补点后重跑 §6 作为对照基线。
