# PHASE3L-WIN：合并 4M、字体微秒打点与内存时间线、ConPTY 随附部署目录

- 日期：2026-11-15（按系统时间）
- 范围：合并 `phase4m-done`；用 4M 新增的 `font_steps_us` / `font_group_us` 取字体数据并回答 4M 的判据；内存时间线（含 perf 锚点对齐）；把官方 ConPTY 随附方案做成可验证的 dist 目录 + zip，并做部署验证、进程核对、体积统计。**未改 `crates/` 下任何代码，未加依赖，二进制不入库。**
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；用户 WezTerm 与安全软件常驻。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 0. 进程与清理（本轮实际动作，全部按 PID）

- 本轮**未结束任何** `OpenConsole.exe` / `conhost.exe` / `wezterm-gui.exe` / `pwsh.exe`。
- 结束过的进程只有本任务自己启动的 `app.exe`，且每个都先核对命令行（含本轮的 dist 路径与 cfg 参数）再按 PID 结束：`7360`、`8240`（`--config examples/windows.toml`，内存采样脚本传错 PID 时残留）、`4880`+其子 shell `7788`（wrap 探针首次挂起）、`8784`/`8628`/`7360`（dist wrap 探针 `CloseMainWindow` 未生效后残留）。
- 测试期间常驻的 `wezterm-gui` 两个实例（PID 2156 启动于 08:33、8568 启动于 11:04，命令行 `start --cwd C:\vt`）**均未触碰**。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4m-done` 实际 hash（`git ls-remote`） | `2d08837d5027c8be57e6edc20c68eae49f9178dd`，`refs/tags/phase4m-done` 只有一行（**轻量 tag，无 `^{}` 行**），`git cat-file -t` = `commit` |
| 合并方式 | `git merge --no-ff phase4m-done`，**0 冲突**，合并 commit `033b6d5` |
| 合并带入 | `PHASE4M-LINUX.md`、`crates/app/src/follow.rs`(+28)、`crates/app/src/lib.rs`(+61/−)、`crates/render/src/lib.rs`(+100/−)、`docs/DECISIONS.md`、`scripts/e2e/run_all.sh` |
| Windows 专属文件 hash（合并前后） | `git ls-files` 中 94 个含 "win" 的文件（排除 `vt-verify-win/`）整体 sha256 **`a8c258e1bb669c36f501383098d42e13ffecf29574fe86c55813893a15d84c65` 前后完全一致** |
| debug 构建 | `build.ps1 -Zig ... ` 通过（`Finished dev profile ... in 30.20s`） |
| release 构建 | 通过，**零 warning**（`Finished release profile ... in 17.90s`，输出中无 `warning:`） |
| clippy（dev，`--workspace --all-targets -D warnings`） | 0 告警 |
| clippy（release，同上） | 0 告警 |
| `cargo test --workspace` | 全绿：各测试二进制 `test result: ok`，合计 0 failed（含 `pty_windows` 21 passed / 13.99s、首个长跑 34.93s、39.03s 等） |
| `cargo tree` 中 `wgpu`、`pollster` | **命中 0**（`grep -cE '^(wgpu|pollster) '` = 0） |
| `cargo test -p app --lib follow` ×10 | 10/10 通过，失败 0 |
| `cargo test -p app --lib paste` ×10 | 10/10 通过，失败 0 |

构建沿用既有入口（`ZIG=C:\vt\toolchain\zig\zig.exe`、`GHOSTTY_SOURCE_DIR=C:\vt\vendor\ghostty-src`、`GHOSTTY_ZIG_SYSTEM_DIR=C:\vt\zig-system`、`CARGO_TARGET_DIR=C:\dev\tty\target`），仅作为命令参数传入，未写入仓库。

## 2. t2 字体微秒打点（4M 新增观测）

### 2.1 一个先决事实：读数受"stderr 重定向方式"支配（已验证）

同一 release 二进制、同一配置，只改 stderr 去向：

| 重定向方式 | first_frame_ms 中位 / 最小 | 单次 `eprintln!` 成本 | 组内 `total_us − (parse+clone)` |
|---|---|---|---|
| PowerShell `Start-Process -RedirectStandardError <file>` | **1962 / 1818 ms** | 22–32 ms（由 `pre_load_us` 直接量出） | 122–162 ms |
| bash `2>file` | **418 / 400 ms** | 0.08–0.22 ms | **1.3–1.9 ms** |

独立探针对照（`vt-verify-win/probe_logcost`，同长度 200 字节行，50 行）：

| stderr 目标 | `eprintln!` 中位 | 裸 `write_all` 中位 |
|---|---|---|
| 文件（bash 直接句柄） | **28 µs** | 9 µs |
| 文件（PowerShell `Start-Process` 重定向） | **6754 µs** | 1933 µs |
| `NUL` | 4 µs | 1 µs |

**结论（已验证）**：`(total_us − steps_us)` 的量级由测试宿主的重定向方式决定——PowerShell 的 `Start-Process -RedirectStandardError` 让每次写花 ~6.8 ms（in situ 22–32 ms），把缺口放大到 120–160 ms；同一二进制在 bash 直接重定向下缺口只有 **1.3–1.9 ms**，剩余部分才是字体处理本身。**因此 3J 的「137 ms 未归属」不是字体解析，而是测试端日志写出**；3E/3K 里 1.5–2.2 s 的首帧同样被这一项抬高（同机 bash 重定向下首帧 418 ms）。
本机测试 stderr 一直是重定向到文件（非控制台），所以这不是"控制台打印"成本，而是**该重定向宿主的写成本**。

### 2.2 主字体面（默认配置 Consolas，bash 重定向，5 次运行，单位 µs，中位/最小）

| style | kind | bytes | ttc | query | face | meta | read | settings | parse | clone | insert | steps |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Regular | file | 459 180 | false | 18/15 | 0/0 | 127/117 | 467/406 | 0/0 | 26 936/24 299 | 6 957/6 450 | 5/5 | 46 568/32 346 |
| Bold | file | 397 896 | false | 21/17 | 0/0 | 271/149 | 346/279 | 0/0 | 22 202/20 843 | 6 966/5 832 | 1/1 | 29 856/28 189 |
| Italic | file | 467 584 | false | 18/16 | 0/0 | 275/274 | 542/494 | 0/0 | 23 900/22 972 | 9 631/6 232 | 1/1 | 40 998/30 499 |
| Bold Italic | file | 407 956 | false | 17/15 | 0/0 | 266/233 | 341/331 | 0/0 | 27 359/25 839 | 9 247/6 436 | 5/5 | 44 842/34 233 |

组级（5 次，中位/最小，µs）：

| group | style | fallback | pre_load | load | tail | total | total − steps |
|---|---|---|---|---|---|---|---|
| 0 | Regular | false | 220/208 | 47 586/33 362 | 0/0 | 47 804/33 636 | 1.2 ms |
| 1 | Bold | false | 97/77 | 30 381/28 612 | 0/0 | 30 503/28 690 | 0.6 ms |
| 2 | Italic | false | 82/74 | 41 410/30 925 | 0/0 | 41 493/31 015 | 0.5 ms |
| 3 | Bold Italic | false | 78/75 | 45 384/34 757 | 0/0 | 45 464/34 836 | 0.6 ms |

四组合计约 **165 ms**（中位），与 3J 的 168 ms 同量级；其中 parse 占 22–27 ms/面、clone 7–10 ms/面。

### 2.3 回退字体面（Microsoft YaHei，`ttc` 集合；把 YaHei 作主字体取得真实数值，5 次运行）

| style | kind | bytes | ttc | query | face | meta | read | settings | parse | clone | insert | steps |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Regular | file | 19 701 556 | **true** | 19/14 | 0/0 | 167/125 | 14 520/11 323 | 0/0 | **584 988/490 171** | **195 045/161 143** | 5/4 | 789 689/696 699 |
| Bold | file | 16 880 832 | **true** | 38/30 | 0/0 | 275/248 | 11 368/10 378 | 0/0 | **505 839/466 889** | **169 643/158 333** | 1/1 | 697 183/679 056 |

组级：group0 total 801 248 µs（total−steps 11.6 ms）、group1 698 574 µs（1.4 ms）；**group2/group3 没有 `font_steps_us` 行**（yahei 无 italic 变体，命中 `face_cache`），但其 `load_us` 分别为 155 602 / 141 756 µs——即纯 `Font::clone()` 成本，每组 **136–156 ms**。

**回答 4M 的 (c)**：一个 19.7 MB 的 CJK 回退面，解析 ~490–585 ms、克隆 ~161–195 ms；同一 face 被后两组复用时，每组仅克隆就要 **135–155 ms**。这解释了"回退字体让启动变慢"的主要构成，且与模板缓存/日志无关。

### 2.4 默认配置下真正走的是哪条回退路径（已验证）

- 默认配置（`examples/windows.toml`，主字体 Consolas ×4 + `fallback = [Microsoft YaHei Regular]`）下，`font_steps_us` **每次运行只有 4 行，全部是 Consolas**，`fallback=false`，`/vt-verify-win/logs/3l-font/bashA-*.err` 5 次合计 20 行、总字节 8 663 080（每次 1 732 616 字节）。
- YaHei 在这条路径上由 `render::load_font_list()`（后台线程、非 `_cached` 入口）加载，**不打点、不经过 face_cache**，`bg_fonts_ms count=3`（YaHei + Monospace + SansSerif 兜底）。
- 因此：默认配置下"回退字体面的 clone/parse"**无法从 4M 打点读出**（未核实项，已入请求栏）；2.3 的数值来自把 YaHei 提为主字体的同族测量（`scripts/windows/e2e/cfg/t6d-yahei-ttc.toml`），是同一文件的同一解析路径，但不等于后台路径的实测值。

## 3. t3 内存时间线

### 3.1 采样方法

`run_3l_mem_timeline.ps1`：app 由 bash 拉起（stderr 直写文件，避免 2.1 的写成本），按 `/proc/<msyspid>/winpid` 取 Windows PID，PowerShell 侧 50 ms 一次采 `WorkingSet64` / `PrivateMemorySize64` 共 5 s，之后每 5 s 到 60 s。`run_3l_fine.ps1`：20 ms 一次、6 s 窗口，并让同一次运行的 `[perf]` 行落在文件里，用 app 自己的 `since_start` 锚点对齐。

### 3.2 结果（各 3 次）

| 配置 | 首点 | 平台值（60 s 末点） | 峰值 |
|---|---|---|---|
| A 默认（Consolas + YaHei 回退） | — | **ws 872–876 MB / priv 890–894 MB** | — |
| B 仅 Consolas（无回退） | — | **ws 189 MB / priv 180 MB** | — |

60 s 内 A/B 均无 ≥20 MB 的阶跃（`|ΔWS| ≥ 20` 检测为空），即 10 s 之后完全平坦。

### 3.3 690 MB 的形状：连续爬升，不是四步台阶

20 ms 采样（A 的 3 次、B 的 3 次；用 `first_frame_ms` / `bg_fonts_ms` 换算到同一时间轴，出生时刻 = 首个 ws < 10 MB 的点）：

| 运行 | first_frame | bg_fonts | 峰值 ws / 时刻 | 末点 ws |
|---|---|---|---|---|
| A1 | 1984 ms | 2502 ms | **1006 MB @ 3434 ms** | 877 MB |
| A2 | 1796 ms | 2442 ms | **1007 MB @ 3902 ms** | 879 MB |
| A3 | 2000 ms | 2450 ms | **1014 MB @ 3440 ms** | 878 MB |
| B1 | 1977 ms | 2092 ms | 190 MB @ 2506 ms | 190 MB |
| B2 | 1976 ms | 2076 ms | 189 MB @ 2527 ms | 189 MB |
| B3 | 1861 ms | 1976 ms | 189 MB @ 2425 ms | 189 MB |

A1 曲线（每 ~100 ms 取样，`vt-verify-win/logs/3l-mem/fine-A1.csv` 全量 160 点）：
`418ms=7MB → 609ms=25 → 908ms=40 → 1512ms=59 → 1602ms=75 → 2409ms=125 → 2502ms=163 → 2814ms=270 → 2907ms=379 → 3001ms=502 → 3216ms=766 → 3318ms=871 → 3607ms=877（此后恒定）`。

**回答"一次性跳升还是分几步"（数据直接支持的表述）**：既不是一次性跳升，也不是 4 个 ~170 MB 的台阶；是**从 `bg_fonts` 到位（约 2.5 s）起、持续约 650 ms 的连续爬升**，每次 20 ms 采样增 10–120 MB，在 3.4 s 左右达到 **~1010 MB 峰值后回落 ~130 MB 稳定在 877 MB**。B 无此段（停在 189 MB）。
不做的归因：本轮**没有**逐段定位这 690 MB 分给谁（字形位图缓存 / fontdue 内部结构 / 渲染器缓存），只记录形状与时刻。

### 3.4 与 4M/4N 对照：解析了几次、每次多少字节

| 配置 | `font_steps_us` 行数/次 | kind | 每次解析字节数 | 回退面加载次数与总字节 |
|---|---|---|---|---|
| A 默认 | **4**（Consolas ×4） | 全部 `file` | 1 732 616 | YaHei 每次 **1 次**（`bg_fonts count=3`，含 2 个系统兜底面），**不走打点路径，字节数未核实**；同文件实际大小 19 701 556 字节 |
| YaHei 主字体（对照） | 2（YaHei Regular/Bold） | `file` | 36 582 388 | 2 次 `from_bytes`，group2/3 命中缓存（克隆两次，见 2.3），`bg_fonts count=2` |

- 默认配置下不会重复解析主字体（`face_cache` 生效：4 组 4 个文件各解析一次）。
- 4M/4N 若在 Linux 上得到"回退面多次解析"的结论，在本机 **Windows 默认配置下不成立**：YaHei 只加载一次，且在后台线程。

## 4. t4 随附部署目录与压缩包

### 4.1 新增 `scripts/windows/package.ps1`

- 全参数化，无绝对路径：`-OutDir`（默认 `<repo>/vt-verify-win/dist`，gitignore 内）、`-TargetDir`（默认 `<repo>/target/release`）、`-ConptyDir`（复用已验证目录）、`-Proxy`、`-NoticePath`、`-ZipName`。
- 任何校验失败即非零退出并清理：实测把 `conpty.dll` 改一个字节后运行 → 打印 `conpty.dll sha256 mismatch: expected e971856f…`、`exit=1`、无 `dist` 目录、无 zip 残留。
- 产物：目录 8 文件 **9 093 735 字节**；`tty-win-x64.zip` **3 436 974 字节**，sha256 **`04b55b7909190d8bfd31c91eb30c2975c084840aef766dcda7aaf64eed987b00`**。
- `MANIFEST.txt`（dist 内逐文件 sha256 + 字节数）：

| 文件 | 字节 | sha256 |
|---|---|---|
| app.exe | 7 854 580 | `e87dc9649f73f6d79d2cce119e5af98763bfc2fa90fa1c77b9b7e345e7bfca6a` |
| conpty.dll | 119 608 | `e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492` |
| OpenConsole.exe | 1 089 336 | `b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775` |
| config.example.toml | 3 495 | `f4d6862c758396497096277ec80636e512b4aa2b89c4d9e555a1357eeb48cba4` |
| LICENSE-ConPTY.txt | 1 116 | `5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b` |
| NOTICE.md | 23 176 | `e7fbaadee6ab20c28b87730a510ee5f5815d8fb4bd88d1d54d282dc2a74c0726` |
| README.md | 1 787 | `ab11d7da1165ff13207660ebd9770d0df6fa3864c1ffc0db04fd4b408290910a` |

`NOTICE.md` 来源：`https://api.github.com/repos/microsoft/terminal/contents/NOTICE.md?ref=f6de9e0618015def1c9dda5b82f9d3abf03eb799`（commit f6de9e0，2026-11-11T18:58:49Z，`main` 当时指向它），内容 sha256 已固定进脚本，上游改动会导致脚本失败而不是悄悄换内容。

### 4.2 上游 NOTICE.md 事实表（逐行读过，只列事实）

| 组件 | 许可（文本自身表述） | 含"随二进制再分发需保留声明"类条款 |
|---|---|---|
| jsoncpp | MIT（`Permission is hereby granted, free of charge…`，`Copyright (c) 2007-2010 Baptiste Lepilleur and The JsonCpp Authors`） | 未见"must reproduce in binary form" |
| chromium/base/numerics | BSD 3-clause 式（`Redistribution and use in source and binary forms…`） | **是**（二进制形式须复现版权声明与免责声明） |
| \{fmt\} | MIT | 未见 |
| interval_tree | MIT（含 `shall be included in all copies…`） | **是**（所有副本须包含版权与许可声明） |
| PCG Random Number Generation | MIT（同上句式） | **是** |
| wyhash | 公有领域声明（`This is free and unencumbered software released into the public domain` 类表述） | 无 |
| stb | 公有领域（同上） | 无 |
| Oklab | MIT（含 included in all copies 句式） | **是** |
| ColorBrewer | Apache-2.0（`Apache-Style Software License for ColorBrewer…` + Apache 2.0 正文引用） | 未见"二进制须复现"；Apache-2.0 自身另有 NOTICE 条款（本文未展开，**未核实**） |
| cmark | MIT + BSD 3-clause（文件内多段） | **是**（两处二进制复现条款） |
| fzf | MIT（`The above copyright notice and this permission notice shall be included in all copies…`） | **是** |
| GSL / Microsoft-UI-XAML / VirtualDesktopUtils / wil | MIT（`This product also incorporates source code from other Microsoft open source projects, all licensed under the MIT license`，wil 附 MIT 全文） | **是**（MIT 全文句式为"shall be included in all copies or substantial portions"） |

- dist 处理：`NOTICE.md` **逐字**随附（sha256 与上游一致），满足"随二进制再分发时保留声明"这一类要求的形式；是否构成完整合规**不作法律结论**（未核实）。
- 未发现任何与再分发冲突的条款（无 GPL/AGPL 类 copyleft 组件出现在该 NOTICE 中；文中只出现一处 LGPL 提及，属"允许为调试而反向工程"的说明，不是许可授予）。
- 仓库自身没有 `LICENSE` 文件、`Cargo.toml` 未声明 license（事实）；dist 的 README 已注明"终端本体在此归档中不带许可证文件"。

### 4.3 部署验证（dist 解压到含空格与中文的路径，从 `C:\Windows` 启动）

路径：`…\vt-verify-win\logs\3l-deploy\tty dist 部署\`；8 个文件齐全。

| 用例 | 启动 | 实际加载的 conpty.dll | 行为 |
|---|---|---|---|
| 1 完整 dist | 成功 | **`…\tty dist 部署\conpty.dll`**（模块列表实测） | 正常出窗、可输入 |
| 2 删除 conpty.dll + OpenConsole.exe | 成功 | **无 conpty.dll 模块** → 系统 conhost | 正常出窗、可输入（回退可用） |
| 3 只删 OpenConsole.exe，保留 conpty.dll | 成功 | `…\tty dist 部署\conpty.dll` | 无崩溃、无错误输出、正常出窗（**未核实**其 pty 宿主是谁：见下） |

- 用例 3 的行为与"Windows 会把 conpty.dll 的宿主解析到别处"一致，但**没有证据表明它退化成了系统 conhost**；本轮没有继续验证其功能等价性（限制）。
- 折行/`ESC[7m` 判据：**像素级判别未取得结论**。在 app 的 120 列窗口下，conhost 与 conpty.dll 都在窗口宽度换行，实测三种布局的"最右暗像素"完全一致（958 px ≈ 120 字符），不能区分；3K 的差别只在**原始字节流**（conhost 物化 CRLF）与"缩放后能否重排"上，本轮没有完成该判别（三次尝试后停止，见 §6）。因此 (2) 的证据链为：**模块路径证明 app 用的是 dist 内那份 conpty.dll** + 该 sha256 已由 3K 的字节探针证明不物化折行、保留 `ESC[7m`、透传 `OSC 104`（同一二进制）。属**推断**，不是本轮新测。

### 4.4 进程核对（每次测试前后记录，未结束任何 OpenConsole/conhost）

| 时点 | OpenConsole PID 集合 | conhost PID 数 |
|---|---|---|
| 测试前 | `328, 9376` | 25 |
| 用例 1 关闭后 | `328, 9376`（**无新增**） | 25（**无新增**） |
| 全部用例结束后 | `328, 9376`（**无新增**） | 25（**无新增**） |

- 结论（已验证）：本轮三次启动、三次关闭之后，`OpenConsole.exe` 与 `conhost.exe` 的 PID 集合**完全没变**——即该 dist 组合在启动/退出过程中没有留下新的宿主进程被观测到；采样点只有"启动前"与"关闭后 2 s"，**运行期间的瞬时宿主未采样**（限制）。
- 无法证明归属的残留：无。`app.exe` 的残留（`7360`/`8240`/`4880`/`8784`/`8628`/`7360`）均已按命令行核对为本任务所起并按 PID 清理。

### 4.5 体积

| 项 | 大小 |
|---|---|
| dist 目录（8 文件） | 9 093 735 字节（≈8.67 MiB） |
| zip（Optimal） | 3 436 974 字节（≈3.28 MiB） |

## 5. 未解决问题

1. 默认配置下后台回退面（YaHei）走 `load_font_list()`，**没有** `font_steps_us`，其 parse/clone 只能从"把 YaHei 提为主字体"的同族测量外推（请求 1）。
2. 690 MB 内存爬升的内部分配归属未定位（只记录形状与时刻，未做归因）。
3. 部署验证中的像素级折行判别未完成（120 列窗口下两种宿主同形）；用例 3（只剩 conpty.dll）的宿主身份与功能等价性未核实。
4. `conhost` 数量为 25（测试前后不变），但这些 conhost 属于用户环境中的其他会话，未逐一核对其归属。

## 6. 对共享代码 / 观测增强的请求

1. **（观测增强，P2）** `render::load_font_list()` 后台回退路径补 `font_steps_us`（或等价打点），并在 `add_background_fonts` 处补一条"并入组"的微秒打点——否则回退面的解析/克隆成本在默认配置下不可见。
2. **（观测增强，P2）** `font_group_us` 建议增加 `cache_hit=true/false` 字段：本轮 group2/3 没有 `font_steps_us` 行却有 135–155 ms 的 `load_us`，只能靠"缺行"反推是缓存命中的克隆成本。
3. **（信息，不改代码）** 测试脚手架侧：`Start-Process -RedirectStandardError` 每次写 ~6.8 ms，会把 `total − steps` 与首帧放大 4–5 倍；建议所有 perf 采集统一走"直接文件句柄重定向"，否则跨平台数据不可比（4M 的判据需据此重读）。
4. 无 `crates/` 改动需求。

## 7. 手动清单（≤3）

1. 在目标机执行 `pwsh -File scripts/windows/package.ps1 -OutDir <dir>`，确认输出的 zip sha256 与本报告一致（同版本输入时）。
2. 部署后从 dist 目录启动 `app.exe --config config.example.toml`；如需回退系统 conhost，删除 `conpty.dll` 与 `OpenConsole.exe` 后重启。
3. 若关注启动耗时的绝对值，用直接文件重定向（`app.exe 2>log`）而不是 PowerShell 的 `Start-Process -RedirectStandardError` 采集 `[perf]` 行。

## 8. 推送与 tag（ls-remote 实际输出）

见文末补记。
