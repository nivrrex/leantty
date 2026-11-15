# PHASE3M-WIN：合并 4N、Arc 字体共享的 Windows 内存验证、启动重测、只剩 conpty.dll 的部署情形

- 日期：2026-11-15（按系统时间）
- 范围：合并 `phase4n-done`；验证 4N t3（`Arc<Font>` 共享）在 Windows 上的内存效果；用**直接句柄重定向**重测启动并修正 3K 的随附 ConPTY 启动代价；核对只剩 `conpty.dll` 的部署情形；改用字节探针判别折行。**未改 `crates/` 下任何代码，未加依赖，二进制不入库。**
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；用户 WezTerm 与安全软件常驻。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- **本轮所有性能与内存数据均用 release 构建**；`[perf]` 日志一律用直接句柄重定向（bash `2>file`、`2>&1 >/dev/null | tee`），**未用** PowerShell `Start-Process -RedirectStandardError` 采集任何数据表（仅作为对照项出现）。

## 0. 进程处理

- 未结束任何 `OpenConsole.exe` / `conhost.exe` / `wezterm-gui.exe` / `pwsh.exe`。
- 只结束本任务自己启动的 `app.exe`，每次先核对命令行（指向 `vt-verify-win\start-S|start-N`、`3m-deploy\dist *`、`examples/windows.toml` 等本任务路径）再按 PID 结束。
- 测试期间 `OpenConsole` PID 集合恒为 `328, 9376`（用户环境），`wezterm-gui` 两个实例未触碰。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4n-done` 实际 hash（`git ls-remote`） | `53db541e27848fe77770dc07a9f1bbf16cf5462f`，只有一行（**轻量 tag，无 `^{}`**），`git cat-file -t` = `commit` |
| 合并 | `git merge --no-ff 53db541`，**0 冲突**，合并 commit `c4b0a48` |
| 合并带入 | `PHASE4N-LINUX.md`、`crates/render/src/lib.rs`(+137/−…)、`crates/render/src/gpu/mod.rs`、`crates/app/src/lib.rs`、`crates/app/src/follow.rs`、`crates/render/tests/{frame_bytes_identical,font_sharing,font_memory}.rs`、`scripts/e2e/font_mem_probe.sh`、`docs/DECISIONS.md` |
| Windows 专属文件整体 sha256（合并前后，沿用 3L 方法） | `git ls-files` 中 99 个含 "win" 的文件（排除 `vt-verify-win/`）整体 sha256 **`84eb92a88e2e78cd48db27df916b705cbf35735278e384755f50f2eaeb0708f6` 前后一致** |
| debug / release 构建 | 均通过；**release 零 warning**（`grep -c '^warning'` = 0） |
| clippy（dev / release，`--workspace --all-targets -D warnings`） | 各 0 告警 |
| `cargo tree` 中 `wgpu`、`pollster` | 命中 **0** |

## 2. t2 测试（4N 新增测试在 Windows 的真实结果）

`cargo test --workspace`：**全绿**，rc=0，各测试二进制 `test result: ok`，无 failed/ignored 例外（`--nocapture` 复核见下）。

| 测试文件 | 用例 | 结果 | 说明 |
|---|---|---|---|
| `crates/render/tests/frame_bytes_identical.rs` | `deterministic_frame_hash_is_stable` | **通过**（2.12–2.22 s） | 实际输出 `frame_hash=507485fb7945fb8f dims=320x192`，**与 Linux 黄金值 `0x5074_85fb_7945_fb8f` 完全相同**；字体为 `DejaVu Sans Mono` 四样式 + `Noto Sans CJK SC` 回退，本机两者均已安装（225 个字族中命中），因此是真实渲染而非缺字体空跑的退化通过 |
| `crates/render/tests/font_sharing.rs` | `same_face_is_shared_not_cloned` | **通过**（1.13 s） | 断言四组内无重复 `Arc` 实例、同 face 指针相等 |
| `crates/render/tests/font_memory.rs` | `measure_font_instance_and_clone_cost` | **通过**（22.24–23.42 s） | 本机实测（详见下表），非 SKIP |

`font_memory` 在 Windows 的实际输出（`--nocapture` 原文）：

| 字体面 | 文件字节 | 实例 MB | clone 增量 MB | clone/实例 |
|---|---|---|---|---|
| DejaVu Sans Mono（小字体） | `C:\Windows\Fonts\DejaVuSansMono.ttf` 340 712 B | 8.9 | 8.9 | 100% |
| ↳ 释放后回落到 | | 0.3（基线 0.3） | | |
| Noto Sans CJK（fallback 候选） | `C:\Windows\Fonts\NotoSansCJK-Regular.ttc` 19 484 784 B | **298.7** | **298.7** | 100% |
| ↳ 释放后回落到 | | 18.6（基线 18.6） | | |
| Noto Sans CJK JP（同一 .ttc 的另一面） | 同上 | 298.7 | 298.7 | 100% |

- 无失败项，无 SKIP。
- 该表是 t3 的量化基础：**一个 19.5 MB 级 CJK `.ttc` 面在 fontdue 里占约 300 MB，克隆是 1:1 深拷贝**——4N 之前 `add_background_fonts` 对每个回退字体做 `cloned()` 并推入**四个样式组**，即 4 份实例。

## 3. t3 内存重测（Arc 共享修复的 Windows 效果）

### 3.1 方法

- app 由 bash 直接拉起（`app.exe --config X 2>file`，直接句柄），Windows PID 经 `/proc/<msyspid>/winpid` 取得；采样器先行就绪（`.ready` 标记）后由 bash 写入 PID，脚本 `run_3m_curve.ps1` 以 20 ms 节奏采 `WorkingSet64`/`PrivateMemorySize64` 5 s，之后每 5 s 到 60 s。
- 实测每样本开销约 20 ms（`Get-Process` 调用本身），即**有效采样间隔约 40 ms**；采样器接入时刻（`attach_at`）记录在每行，1.0–2.4 s 之间，**进程创建后最初 1 s 不在采样覆盖内**（该段只有约 90 MB 的增长，见 3.3）。
- 对齐用同一次运行的 `[perf]` 锚点（`first_frame_ms`、`bg_fonts_ms`、`fontdb_ms`）。

### 3.2 结果（各 3 次；µs 级 perf、MB 级内存，全部 release）

| 配置 | 运行 | 平台 ws / priv | 峰值 ws @ 时刻* | 首点 ws | first_frame | bg_fonts | `font_steps_us` 面数 | bg_fonts count |
|---|---|---|---|---|---|---|---|---|
| 默认（Consolas ×4 + YaHei 回退） | A1 | **252 / 244** | 252 @ 361 ms | 149 | 535 | 1130 | 4 | 3 |
| 默认 | A2 | **252 / 244** | 252 @ 20015 ms | 149 | 417 | 1317 | 4 | 3 |
| 默认 | A3 | **252 / 244** | 252 @ 221 ms | 203 | 307 | 895 | 4 | 3 |
| 仅 Consolas（无回退） | B1 | **83 / 72** | 83 @ 12 ms | 83 | 415 | 563 | 4 | 2 |
| 仅 Consolas | B2 | **83 / 72** | 83 @ 12 ms | 83 | 421 | 541 | 4 | 2 |
| 仅 Consolas | B3 | **83 / 72** | 83 @ 10 ms | 83 | 411 | 547 | 4 | 2 |
| 仅 YaHei Regular（四样式同面，单实例） | YS1 | **221 / 212** | 221 @ 535 ms | 191 | 838 | 956 | 1 | 2 |
| 仅 YaHei Regular | YS2 | **220 / 211** | 220 @ 30034 ms | 195 | 1036 | 1170 | 1 | 2 |
| YaHei Regular+Bold（两实例） | Y1 | **371 / 365** | 371 @ 1170 ms | 208 | 1593 | 1703 | 2 | 2 |

\* 时刻为采样器自身时间轴的毫秒值；`attach_at` 见 §3.4 表。

### 3.3 与 3L 并列对照（回答"降到多少"）

| 指标 | 3L（深拷贝，4 份实例） | 3M（Arc 共享） | 变化 |
|---|---|---|---|
| 默认配置平台 ws | **877–879 MB** | **252 MB** | **−625 MB（−71%）** |
| 默认配置平台 priv | 890–902 MB | **244 MB** | −650 MB 左右 |
| 默认配置峰值 ws | **1006–1014 MB @ 3.4–3.9 s** | **252 MB（无过冲）** | 峰值消失，峰值=平台值 |
| 仅 Consolas 平台 ws | 189 MB | **83 MB** | −106 MB |
| 爬升段 | `bg_fonts`(≈2.5 s) 起约 **650 ms** 内 +740 MB，之后回落 ≈130 MB 到平台 | `bg_fonts`(0.5–1.3 s) 附近约 **+100 MB**（149→252），**无回落** | 爬升段基本消失 |

- 3L 的 3 次 A 平台值 872/874/876 MB，3M 三次均为 **252 MB**，离散度也从数 MB 降到 0。
- 3.3 的"仅 Consolas"基线同时从 189 MB 降到 83 MB：4N 之前 `Monospace`/`SansSerif` 两个系统兜底面同样被 `cloned()` 进四组；本机这两个 generic 解析到含 CJK 的字体，故基线也一起下降（**工程判断**，未逐一验证具体 face）。

### 3.4 爬升段形状（A1 全量曲线，节选）

`fine-A1.csv`（1 529 点，覆盖到 62 s）：`t=14 ms→149 MB（attach 时刻）`、`64 ms→170`、`120→192`、`211→227`、`323→246`、`360→252`，此后 60 s 恒为 252 MB。即：**接入后在约 350 ms 内从 149 MB 升到 252 MB 并稳定**，`bg_fonts`（1130 ms）落地时没有额外阶跃。

### 3.5 是否仍明显高于「基线 + 一份 YaHei 实例」

- 本机单 YaHei Regular 实例的实测量级：YS = **220–221 MB**（含基线 83 MB）⇒ 单面约 **137 MB**。
- 默认配置 A = **252 MB** ≈ 83（基线）+ 137（一份 YaHei）+ **32 MB 未解释**。
- 判定（数据表述）：A 与"基线 + 一份 YaHei"相差约 32 MB（约 13%），**不存在第二份、第三份实例的量级（那会是 +137 MB 以上）**。这 32 MB 未归因，按任务要求只记录。

## 4. t4 启动重测（直接句柄重定向）

### 4.1 S 与 N 各 10 次（默认配置，bash `2>file` 直接句柄）

| 条件 | n | first_frame 中位 / 最小 / 最大 | first_pty_byte 中位 / 最小 |
|---|---|---|---|
| **S** 系统 conhost（目录内无 conpty 两文件） | 10 | **419 / 395 / 524 ms** | 726 / 662 ms |
| **N** 官方 ConPTY 1.25.261109001（两文件与 app.exe 同目录） | 10 | **524 / 416 / 552 ms** | 529 / 446 ms |

- 逐次 first_frame（S）：456, 441, 419, 406, 400, 524, 419, 395, 453, 406。
- 逐次 first_frame（N）：见 `vt-verify-win/logs/3m-start/N-*.err`。

### 4.2 重定向方式对照（同一二进制 start-S、默认配置）

| 重定向方式 | n | first_frame 中位 | 最小 | 最大 |
|---|---|---|---|---|
| **file（直接句柄，bash `2>file`）** | 10 | **419 ms** | 395 | 524 |
| 管道 tee（`2>&1 >/dev/null \| tee file`，无磁盘写） | 5 | **399 ms** | 296 | 423 |
| PowerShell `Start-Process -RedirectStandardError` | 5 | **1772 ms** | 1712 | 1805 |
| `NUL`（`2>/dev/null`） | 5 | **不可读**：日志被丢弃，`first_frame_ms` 无从获取（原理性限制，非失败） | — | — |

- 结论（已验证事实）：PowerShell 的重定向使首帧中位 **1772 ms**，直接句柄 **419 ms**、管道 **399 ms**——**约 4.2–4.4 倍**。3L §2.1 的量级（1962 vs 418 ms）在本轮复现。
- 因此 3C–3L 期间用 PowerShell 重定向采集的启动数据（例如 3K 的 S 1673 / N 1787 / W 1878 ms 中位）**绝对值偏高，只能横向比、不能与直连数据混用**。

### 4.3 修正 3K 的 ConPTY 启动代价

- 3K（PowerShell 重定向）：S 中位 1673 ms、N 中位 1787 ms ⇒ **+110 ms**。
- 3M（直接句柄）：S 中位 419 ms、N 中位 524 ms ⇒ **+105 ms**。
- 结论（数据）：**+105 ms 的差值在两种采集方式下一致**，"官方 ConPTY 首帧比系统 conhost 慢约 0.1 s"这一结论**仍然成立**；但 3K 记录的**绝对首帧（1.6–1.9 s）不成立**，真实量级是 **0.4–0.5 s**。差值同时体现在最小样本（395 vs 416 ms，+21 ms），说明差值本身也有较大波动（+21 ~ +105 ms 视样本而定，中位差 +105 ms）。

## 5. t5 只剩 `conpty.dll` 的部署情形（3L §4.3 用例 3）

用 3L 的 `tty-win-x64.zip` 解压到 `…\3m-deploy\dist full\` 与 `…\3m-deploy\dist dll only\`（**路径含空格**），后者仅删除 `OpenConsole.exe`。

### 5.1 子进程树（`Get-CimInstance Win32_Process`，只记录不结束）

| 用例 | app PID | 子进程 | 命令行原文 |
|---|---|---|---|
| 完整 dist | 4544 / 3476 | `OpenConsole.exe` pid 1408 / 11340 | `"<dist full>\OpenConsole.exe" --headless --width 120 --height 30 --signal 0x3d4 --server 0x3cc` |
| 完整 dist | 同上 | `cmd.exe` pid 11388 | `C:\Windows\system32\cmd.exe /K "chcp 65001"` |
| **仅 conpty.dll** | 3140 / 6876 | **`conhost.exe`** pid 7800 / 11092 | `\\?\C:\Windows\system32\conhost.exe --headless --width 120 --height 30 --signal 0x3e0 --server 0x3d8` |
| 仅 conpty.dll | 同上 | `cmd.exe` / `pwsh.exe` | 与完整 dist 相同形态 |

### 5.2 判定

- **功能是否正常**：app 正常启动、窗口出现、存活 15 s 以上、无退出；两个用例的子进程树结构一致（宿主 + shell）。
- **是否静默回退到系统 conhost**：**是（已验证事实）**。`dist dll only` 下 pty 宿主是 `C:\Windows\system32\conhost.exe --headless …`，而不是 dist 内的 `OpenConsole.exe`；即 `conpty.dll` 虽然被加载（3L 用模块列表证明过它被映射），**实际宿主仍是系统 conhost**。这与"两文件必须成对部署"一致，且与 t6 的行为判据吻合（系统 conhost 会物化折行）。
- **是否有错误输出**：**无**（stderr 直接句柄重定向到文件，两种用例都没有任何错误行，只有正常的 `[perf]` 行）。用户不会有任何提示——属于部署文档必须写明的情形（README 已写"两文件须同目录"）。
- **注入标记文件**：**未核实**。配置型注入无效——app 会把配置里的 `terminal.shell.args` 覆盖为自带的 `/K "chcp 65001"`（子进程命令行原文为证），因此写不进自定义命令；合成按键注入在**对照组（完整 dist）也没有产生标记文件**，即注入本身未生效（`focus=True`、`assert=True`，但标记 15 s 内未出现，两用例一致）。按"每个依赖输入注入的测试必须有注入生效证据"的规则，**该次输入结论作废，判为未核实，不作为产品缺陷**。停止条件"出现输入无效/挂起"因此**未触发**（无法区分产品与注入问题）。
- 新增观察（事实）：**`terminal.shell.args` 在 Windows 上被忽略**，见请求栏。

## 6. t6 折行字节级判别（不用像素）

原始读端探针 `vt-verify-win/probe_ptyin/target/release/ptyin.exe` + `wrap200.ps1`（输出 200 个 `A` 的单行），把对应 conpty 两文件放到探针目录旁实现宿主切换，读取原始字节：

| 宿主 | 原始字节数 | 分段长度 | 是否物化为 CRLF |
|---|---|---|---|
| **S** 系统 conhost（探针目录无 conpty 两文件） | 296 | **80 / 80 / 43** | **是（物化）** |
| **N** 官方 ConPTY 1.25.261109001（来自 dist 的同 sha256 文件） | 297 | **200** | 否 |
| **W** WezTerm 20240203 sideload | 311 | **200** | 否 |

- 日志：`vt-verify-win/logs/3m-seq/wrap-{S,W,N}.txt`（含 `raw_hex`）。
- 与 3K 结论一致：系统 conhost 物化折行，官方 1.25 与 WezTerm 都不物化。3L 的像素判别之所以失败，是因为在 120 列窗口下两种宿主都按窗口宽度显示，视觉同形；**字节层才是判据**。

## 7. 未解决问题

1. t5 的**输入路径**（键入命令是否被执行）未核实：配置型注入被 `shell.args` 覆盖，合成按键在对照组也失效。需要一条不依赖合成输入的注入通道（见请求 2）。
2. 默认配置平台 252 MB 比"基线 83 + 一份 YaHei 137"高约 **32 MB**，未归因（按任务要求只记录）。
3. t3 采样未覆盖进程创建后最初约 1 s（`attach_at` 1.0–2.4 s），该段增长约 90 MB（依据 3L 同时段曲线）。
4. N 相对 S 的首帧差值在不同样本间波动较大（+21 ~ +105 ms），未拆分来源（conpty.dll 加载 vs 宿主进程启动）。

## 8. 对共享代码 / 观测的请求

1. **（功能，P2）`terminal.shell.args` 在 Windows 被丢弃**：子进程命令行为 `cmd.exe /K "chcp 65001"`，配置里写的 `args` 没有出现。若这是"为 UTF-8 代码页强制覆盖"的有意设计，应在文档/配置校验处明示；若非有意，应改为"追加"而不是"替换"。状态=已验证事实（子进程命令行证据），未擅改。
2. **（观测增强，P2）** 部署形态缺自检：只剩 `conpty.dll` 时静默回退到系统 conhost，无任何日志。建议在 `load_conpty()` 成功但宿主不是同目录 `OpenConsole.exe` 时打一条 warn（不改变行为）。
3. **（沿用 3L 请求 1）** 后台回退路径 `load_font_list()` 仍无 `font_steps_us`；4N 之后该路径改为 Arc 共享，"回退面到底占多少内存"只能靠主字体测量外推（本轮用 YS/Y1 兜出了数量级）。
4. **（测试脚手架，不改产品代码）** perf 采集统一用直接句柄重定向；`Start-Process -RedirectStandardError` 只能用于非计时场景。

## 9. 手动清单（≤3）

1. 部署 dist 时务必保持 `app.exe`、`conpty.dll`、`OpenConsole.exe` 同目录；只留 `conpty.dll` 不会报错，但会静默用系统 conhost（折行会被物化、`ESC[7m` 被改写）。
2. 需要可信启动数字时用 `app.exe --config … 2>log`（或 `| tee log`），不要用 PowerShell 的 `Start-Process -RedirectStandardError`。
3. 若关注内存，默认配置（含 CJK 回退）本机空闲约 252 MB；去掉 `font.fallback` 约 83 MB。

## 10. 推送与 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `f475a8467c8913d94e28c2a2d3fcda6c0f808b13` |
| `refs/tags/phase3m-win-done`（tag 对象） | `bea283fd7d715c18058e28024ef453eb235274a9` |
| `refs/tags/phase3m-win-done^{}`（解引用 commit） | `f475a8467c8913d94e28c2a2d3fcda6c0f808b13` |

- `git push origin win`：`65a91d0..f475a84  win -> win`；tag 新建后 push（`* [new tag]`）。未 force、未移动任何已有 tag。
- 提交内容：`PHASE3M-WIN.md`、`docs/WIN-NOTES.md`（6 条 3M 事实 + 请求 1/2/3）、`vt-verify-win/` 下 3M 探针与日志、3 个新 cfg（`t6d-yahei-ttc` 沿用、`t6e-yahei-single`、`wrap200-cmd/win`）、`.gitignore`（3m-deploy 二进制目录、start-S/N 暂存目录）。**无二进制入库**（提交前检查 .exe/.dll/.zip/.png 命中 0）。
