# PHASE3H-WIN：阶段 4H Windows 复验（启动细粒度打点 + 粘贴主线程停顿）

- 日期：2026-11-13（按系统时间）
- 范围：合并 `phase4h-done`；用 4H 新增打点拆开首帧缺口；在真实 Windows 路径量化粘贴时的主线程停顿；达标后在 `crates/pty/src/windows.rs` 实现 `take_writer`。未改共用逻辑、未加依赖、未改任何 trait 现有签名。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；Zig 0.17.1；portable-pty 0.8.1。测试期间本机另有用户自己的 WezTerm（PID 6504）与安全软件常驻，启动数据噪声偏大（已如实记录）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 1. t1 合并（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/tags/phase4h-done` | `fddc97ba1b5854ca3cf73f3dbf3eb241ecc4266d` |
| `refs/tags/phase4h-done^{}` | **无此行** —— 轻量 tag（`git cat-file -t` = commit） |

- 合并：`Merge made by the 'ort' strategy`，提交 **524fdb3**，0 冲突。
- Windows 专属文件 hash 在合并中未变（`windows.rs` 96416daa、`pty_windows.rs` c3bb4b20、`WIN-NOTES.md` 2286072a、`.cargo/config.toml` 311a5385）；`windows.rs` 的唯一改动是 t4 授权的 `take_writer`，单独提交 **128c2bc**（`win:` 前缀）。

## 2. t2 构建与测试

| 项 | 结果 |
|---|---|
| debug 构建 | rc=0，12.3 s |
| release 构建 | rc=0，11.5 s，**零告警**（含 take_writer 之后重新构建） |
| clippy dev / release `--all-targets -D warnings` | 均 rc=0（3h-clippy-dev/rel，以及 take_writer 后重跑的 -dev2/-rel2） |
| cargo test --workspace | rc=0，40 个测试目标全绿（take_writer 后重跑仍绿） |
| `cargo test -p render --test inverse_default_color` | **3 passed**（palette_values_are_distinguishable、inverse_with_explicit_37_uses_normal7_as_background、inverse_without_explicit_color_uses_primary_foreground_as_background）——render 层不经 ConPTY，符合预期 |
| app 写线程单测 | 7 个中 6 过、1 个 flaky：`writer::tests::paste_queue_respects_cap` **10 次里 3 次失败**（队列上限断言与写线程并发消费赛跑），属**共享代码的竞态型测试**，未改，写入请求 |
| cargo tree（`--target x86_64-pc-windows-gnu -e normal`） | wgpu 命中 **0**、pollster 命中 **0**；134 行依赖 |

## 3. t3 启动缺口拆分（release，默认配置，5 次）

采集 4H 全部新打点（原始值均为 `since_start` 毫秒；第一次运行 1381、第二次 6414、第三次 4653、第四次 1318、第五次 1395 —— 本机噪声很大）。

| 打点 | 5 次中位 | 5 次原始值 |
|---|---|---|
| config_ms（@结束） | 29.3 | 29 / 27 / 2789 / 29 / 29 |
| app_new_ms | 54.9 | 55 / 50 / 2832 / 50 / 70 |
| event_loop_ms | 88.8 | 89 / 82 / 2864 / 78 / 108 |
| fontdb_ms | 198.9 | 200 / 169 / 2963 / 158 / 199 |
| font_seg 查询/读/解析（每组） | query ≈ 0.02–0.07 ms、read 0.6–1.1 ms、parse 22–30 ms | 见 3h-perf 日志 |
| font_cache_ms（每组） | ≈ 7–9 ms | |
| font_ms（每组合计） | 139–155 ms | |
| cpu_groups_ms | 1009.9 | 1010 / 6036 / 4265 / 920 / 985 |
| cpu_renderer_ready_ms | 1036.0 | 1036 / 6060 / 4290 / 945 / 1009 |
| cpu_state_ms | 1055.5 | ≈ +20 ms/段 |
| pre_window_ms | 1074.2 | 1074 / 6098 / 4330 / 995 / 1049 |
| window_ms | 1165.7 | 1166 / 6198 / 4423 / 1097 / 1132 |
| ime_ms | 1192.5 | 1192 / 6225 / 4455 / 1131 / 1163 |
| renderer_build_ms | 1212.3 | 1212 / 6246 / 4475 / 1150 / 1184（backend=cpu，复用已建渲染器 → 0.0 ms） |
| renderer_ready_ms | 1235.0 | 1235 / 6266 / 4497 / 1176 / 1210 |
| surface_ms | 1259.2 | 1259 / 6287 / 4519 / 1199 / 1244 |
| vt_new_ms | 1280.0 | 1280 / 6302 / 4540 / 1220 / 1267 |
| pty_spawn_ms | 1305.1 | 1305 / 6345 / 4580 / 1248 / 1297 |
| first_pty_byte_ms | 1359.0 | 1354 / 6423 / 4627 / 1293 / 1359 |
| pre_first_submit_ms | 1380.0 | 1367 / 6387 / 4640 / 1304 / 1380 |
| first_frame_ms | 1381 | 1381 / 6414 / 4653 / 1318 / 1395 |

**分段（取无抖动的第 4 次运行，1318 ms，逐段差值）**：

| 段 | 时长 | 占比 |
|---|---|---|
| 进程启动 → config 结束 | 29 | 2% |
| config → 事件循环就绪（app_new + event_loop） | 49 | 4% |
| **fontdb 扫描** | 69 | **5%** |
| **4 组字体加载（query+read+parse+cache+每面 139–155 ms）** | **675** | **51%** |
| 字体结束 → cpu_renderer_ready（cpu_groups 收尾 + cpu_state） | 112 | 8% |
| cpu_renderer_ready → pre_window（窗口前准备） | 50 | 4% |
| **窗口创建（window_ms）** | **102** | **8%** |
| window → renderer_ready（ime 6 + renderer_build 0 + 收尾） | 79 | 6% |
| renderer_ready → surface → vt_new → pty_spawn | 72 | 5% |
| pty_spawn → first_pty_byte | 45 | 3% |
| first_pty_byte → pre_first_submit → first_frame | 25 | 2% |

**回答"3E/3F-b 的 ≈362 ms 与 ≈120 ms 由谁构成"**：

- **≈362 ms 缺口 = 4 组字体的逐面处理**（本机 675 ms，机器空闲时约 360 ms）：其中 **fontdb 扫描仅 69 ms**、**文件读取每组 0.6–1.1 ms**、**fontdue 解析每组 22–30 ms**，剩下的每组约 100 ms 落在 `font_ms` 与 `font_seg+font_cache` 之差里（同一面注册后的字形/度量准备工作），**无打点**。占比最大的是这一段（51%）。
- **≈120 ms 缺口 = 窗口创建（102 ms）+ ime（6 ms）+ 渲染器就绪前后的 20 ms 级过渡**，全部落在 window → renderer_ready 区间。
- **仍有 >50 ms 未覆盖**：4 组字体里每组约 100 ms、合计约 400 ms 无打点（`font_ms` 内部，字体解析之后到该面可用的部分）。已写入请求栏（请求 1），未做任何优化。

- 附注：本轮绝对值（1381 ms 中位）明显高于 3F-b（781 ms），本机同时运行用户自己的 WezTerm（CPU 时间 23390 s）与安全软件；同一代码在 4 次运行中最好成绩 1318 ms，因此以**相对构成**为准。

## 4. t4 粘贴主线程停顿（真实路径）

方法：release app，前台运行 `ping -t/-n`（不读 stdin），剪贴板用 .NET 设置，粘贴由**对自窗口右键**触发配置的 Paste 绑定；粘贴窗口内每 40 ms 注入一次鼠标移动驱动事件循环；读取 `APP_PERF_FRAMES` 的 `loop_gap_ms`（5 s 窗口最大值），只统计粘贴开始之后的行。

### 4a 量化（实施前）

| 数据量 | 3 次 `loop_gap_ms` 最大值 |
|---|---|
| 1 MB | **1104 / 1114 / 1111 ms** |
| 5 MB | **1093 / 1108 / 1099 ms** |

- 已验证事实：最大停顿 ≥100 ms，且 **1 MB 与 5 MB 基本相同**（差别 <2%），说明停顿来自"前台程序不读 stdin 的持续时间"而不是数据量。

### 4b 判定

判据满足 → **实施**。

### 4c 实施（`crates/pty/src/windows.rs`，提交 128c2bc）

```rust
fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
    // 与 unix 语义一致：把 ConPTY 输入管道的写端移交给调用方（app 的写线程），
    // 之后 writer() 返回 NotSpawned。spawn 时已从 master 取出该写端，
    // 因此不需要再从 master 拿一次（避免两个写端并发写同一管道）。
    self.writer.take()
}
```

- 未改 trait 签名（`take_writer` 是既有带默认实现的方法，默认返回 None）；`writer()` 在取走后仍返回 `NotSpawned`（与 unix 一致）；`close()` 原先就是 `writer = None; master = None;` 再 kill+wait，契约不变；未加依赖。

### 4d 门禁

| 门禁 | 结果 |
|---|---|
| 同场景最大停顿 < 100 ms | **58 / 71 / 60 / 59 / 55 ms**（5 次，均 < 100） |
| Ctrl-C 中断 ping 且未写出的粘贴被丢弃 | 5 MB 粘贴后立即 Ctrl-C：子进程只收到 **123 130 字节**（远小于 5 242 880），app 存活 |
| 1 MB 粘贴在正常读取的 shell 下字节精确 | **1048576 字节**（5.7 s） |
| 粘贴进行中关闭窗口 | WM_CLOSE 后 **1635–2496 ms 内退出**，无残留（只检查自己启动的 PID） |
| `pty_windows` 既有 21 个测试 | **21 passed / 0 failed**（13.7 s） |
| 回退 `take_writer`（返回 None）后停顿用例必须变红 | **1135 / 1086 ms**（重新变红），恢复实现后回到 55–59 ms |

- 说明：Ctrl-C 门禁最初用 dump 判定"提示符返回"在 release 下永远为假（release 无 `APP_DUMP_SNAPSHOT` 钩子），改用"子进程实际收到的字节数"作为判据；`echo` 类屏幕断言不适用于 5 MB 灌入后的回显洪流。

## 5. 未解决问题

1. 字体加载内部每组约 100 ms（4 组共约 400 ms）仍无打点，>50 ms（请求 1）。
2. `writer::tests::paste_queue_respects_cap` 在本机 10 次里失败 3 次（共享测试赛跑，非产品缺陷，请求 2）。
3. 启动绝对值受本机负载影响（1381 ms vs 3F-b 的 781 ms），跨版本比较只能看构成比例。

## 6. 对共享代码的请求

1. **请求 1（观测增强）**：`font_ms`（每面 139–155 ms）与已打的 `font_seg_ms`（parse 22–30 ms）+ `font_cache_ms`（7–9 ms）之间每组仍有约 100 ms 无打点，是首帧缺口的最大单项（4 组约 400 ms）。建议在"字体解析完成 → 该面字形/度量准备完成"之间补一个点。
2. **请求 2（测试稳定性）**：`crates/app/src/writer.rs` 的 `paste_queue_respects_cap` 依赖"写线程尚未消费"的时序，在快机器上会误判；建议改为可控 sink（阻塞消费）或直接断言 `queued_bytes() <= cap` 而不要求出现拒绝。
3. **平台限制登记（来自 3G，按要求写入）**：系统 conhost 会把 `ESC[7m` 物化为 `ESC[30m`+`ESC[47m`（因此默认色反色块显示为 `normal[7]` 而不是 `primary.foreground`）；`OSC 104` 被两种 ConPTY（系统与 WezTerm）丢弃，`OSC 4` 改色后无法复位。

## 7. 交付物与证据

- 代码：`crates/pty/src/windows.rs`（take_writer，提交 128c2bc）。
- 脚本：`vt-verify-win/{run_3h_paste,run_3h_gate,run_3h_drop,run_perf3c}.ps1`。
- 日志：`vt-verify-win/logs/{3h-clippy-dev,3h-clippy-rel,3h-clippy-dev2,3h-clippy-rel2,3h-test,3h-test-final,3h-tree,3h-perf}.log`；`vt-verify-win/logs/phase2b-win/p3-default-*.err`（完整时间戳序列）、`t4-*`、`gate-*`、`drop-probe.txt`。
- 未入库：`perfdata/`、任何 conpty.dll/OpenConsole.exe。

## 7.5 推送与打 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `2ea9cdd7e196582a351e0f35aa7f8ade4d7af427` |
| `refs/tags/phase3h-win-done`（tag 对象） | `0fe3b7f5e88eb0283d9f2991cc55dff6ba5cda8b` |
| `refs/tags/phase3h-win-done^{}`（解引用 commit） | `2ea9cdd7e196582a351e0f35aa7f8ade4d7af427` |

- 提交序列：`524fdb3`（合并 phase4h-done）→ `128c2bc`（win: take_writer）→ `2ea9cdd`（本轮文档/脚本）。
- `git push origin win`：`e3a16ef..2ea9cdd  win -> win`；tag 新建后 push（`* [new tag]`）。全程无 force，未移动任何已有 tag。

## 8. 手动清单（≤3）

1. 在空闲机器上重跑 §3 的 5 次采集，确认字体组那 51% 的占比是否稳定（本机有用户自己的 WezTerm 与安全软件常驻）。
2. 用真实使用场景（`ping -t` 前台 + 大段粘贴）体感确认不再卡顿，并确认 Ctrl-C 后不再吐出大量残留字符。
3. 若后续给字体组内部补打点，用本报告的 675 ms（第 4 次运行）作为对照基线。
