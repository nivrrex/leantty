# PHASE2D-WIN：阶段2C/2D Windows 复验交付

- 日期：2026-11-12（按系统时间）
- 范围：合并 Linux 阶段2D（tag phase2d-done = b227835，与补充说明一致）后在 Windows 复验；复验 2C 功能项、回跑 2B 与 2A 回归、做 pwsh 崩溃判别实验、测 release 启动耗时。仅 CPU，未做 GPU，未进入阶段3。
- 基线：win 合并前 3811f91；合并为快进（main 已含 win 工作），合并后 HEAD = b227835。
- 结论分级：已验证事实 = 本机实测；工程判断 = 基于事实推断；待验证假设 = 未证实。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（windows-gnu）；Zig 0.17.1；portable-pty 0.8.1；pwsh 7.6.6 + PSReadLine 2.4.5。本机未安装 Alacritty 与 Windows Terminal（Get-AppxPackage 无 Microsoft.WindowsTerminal，PATH 无 wt.exe，亦无 alacritty.exe）。

## 1. 合并与构建

| 项 | 结论 | 证据 |
|---|---|---|
| phase2d-done 核对 | `git rev-parse phase2d-done` = b227835afbc5bf3f97a9ce76aca7d8193e1ab03a | 与补充1 要求的 b227835 一致 |
| merge | 无冲突，快进到 b227835 | `git merge phase2d-done` 输出 Fast-forward；无 UU/AA/DD |
| Windows 既有修复保留 | `clear_console_ctrl_ignore` 仍在 crates/pty/src/windows.rs:37；arboard 的 cfg(unix)/cfg(not(unix)) 拆分仍在 clipboard.rs:64-95 | grep |
| debug 构建 | rc=0，19.9s | vt-verify-win/logs/2d-build-debug（同次调用输出） |
| release 构建 | rc=0，26.3s | 同上 |
| cargo test --workspace | rc=101：仅 config `path_priority_cli_env_default` 失败（Linux 修的是 temp 目录构造，XDG 断言在 Windows 仍不成立）；其余 5 个测试二进制全绿（41/3/3/0/…） | vt-verify-win/logs/2d-test.log |
| clippy --all-targets -D warnings | rc=0，13.6s，无 warning | vt-verify-win/logs/2d-clippy.log |

`path_priority_cli_env_default` 的实际断言（记录）：

```
path "C:\\Users\\Admin\\AppData\\Roaming\\config-339e4af65e7b090f\\config-339e4af65e7b090f.toml"
  should start with "C:\\Users\\Admin\\AppData\\Local\\Temp\\xdg-test"
```

原因：`default_config_path()` 在 Windows 走 APPDATA（不读 XDG_CONFIG_HOME），而测试设置 XDG_CONFIG_HOME 后断言路径位于其下。属共享测试的平台假设，未改（请求 1）。

## 2. e2e 回归（2A + 2B）

| 套件 | 结果 | 证据 |
|---|---|---|
| run_all.ps1（2A 全矩阵，含 i/j/k/l/m Ctrl-C） | 14 PASS / 0 FAIL（第 2 次；第 1 次 case a 因焦点竞态 FAIL，重跑转绿） | vt-verify-win/logs/2d-runall-1.log、2d-runall-2.log |
| run_2b_clip.ps1（剪贴板/选区/1MB/PRIMARY） | 11 PASS / 0 FAIL | vt-verify-win/logs/2d-clip-2.log |
| run_2b_mouse.ps1（键绑定/滚轮/滚动条/SGR） | 6 PASS / 0 FAIL | vt-verify-win/logs/2d-mouse-1.log |

2B 的 1MB 断言按补充3 口径改为精确值：`n4a-paste-1mb-idle` 静止粘贴 ⇒ **bytes=1048576**（2D.2 排空修复在 Windows 生效，约 7s 完成）；`n4b` 带鼠标事件同值。

## 3. 2C 复验（run_2c_win.ps1，8/8 PASS）

| 用例 | 结论 | 关键数值 |
|---|---|---|
| c1 复制换行语义 | 三星短文本硬换行保留；长行在**控制台宽度处被 ConPTY 物化为 CRLF**，故 Windows 上不存在软折行拼接 | rA=3 rB=10 newlines=True；探针实测 200 个 A 在 120 列下被拆成 A×120 + CRLF + A×80（probe_ptyin 原始 hex） |
| c2a 选区跨滚动一致 | 上滚 40 行后选两行，滚动往返再复制，文本一致 | len=7 same=True multiLine=True |
| c2b 跨视口拖选自动滚动 | 指针停在底部选择带内自动滚动，复制跨多屏 | lines=69 > viewport=30 |
| c3 IME 预编辑叠加 | 叠加区像素与无预编辑不同、dump 不含预编辑文本 | diffPixels=125~7806，dumpHasPreedit=False |
| c4a OSC 0 标题 | 子进程发 `ESC]0;T123 BEL` 后窗口标题变 T123 | title='T123' |
| c4b dynamic_title=false | 发 T999 后标题不变 | title='app.exe' |
| c4c window.position | 窗口矩形正好落在 (60,50) | winRect=60,50 dx=0 dy=0；client 偏移 8,31（DPI 边框） |
| c5 10MB 粘贴并发键入 | 字节精确 = 10MB+20，标记字符 M/R/K 各 5 个全部到达 | bytes=10485780 target=10485780 M=5 R=5 K=5 |

## 4. pwsh 崩溃判别实验（只记录）

**首要结论（已验证事实）**：本轮同一套矩阵**未复现** `PHASE2B-WIN.md` 记录的 FailFast。`probe_pwsh` 的 flags 0/1/2/4/6/7 × 尺寸 80x24/120x30/200x50 共 18 组合全部 `alive`、无 `totalWidth`、无 `Process terminated`；app 以 `shell=pwsh.exe` 启动也正常（banner + 提示符，alive=True，dump 7555 字节）。

| # | 变体 | 结果 | 退出/状态 |
|---|---|---|---|
| 0 | `pwsh -NoProfile`（基线，18 组合） | 未崩 | alive（窗口结束时由 probe kill） |
| 1 | `-NoProfile -NonInteractive -Command "echo hi"` | 未崩 | exit 0，73 字节 |
| 2 | `-NoProfile -NoExit -Command "Remove-Module PSReadLine"` | 未崩 | alive，109 字节 |
| 3 | `-NoProfile -NoExit -Command "Set-PSReadLineOption -PredictionSource None"` | 未崩 | alive，162 字节 |
| 4a | `TERM=xterm-256color` | 未崩 | alive，221 字节 |
| 4b | `NO_COLOR=1` | 未崩 | alive，221 字节 |
| 4c | `COLUMNS=120` | 未崩 | alive，221 字节 |
| 5 | 版本 | pwsh 7.6.6；PSReadLine 2.4.5 | `$PSVersionTable` / `Get-Module -ListAvailable` |
| 6 | 本机终端 | 无 Alacritty、无 Windows Terminal | 只读查询，未运行、未改配置 |

- 因基线本身不崩，PSReadLine 相关假设在本轮**既不能证实也不能推翻**：所有实验臂都落在"未崩溃"一侧，无法区分是哪个变量起作用。
- 与 2B 记录的差异（同一 probe、同一 flags 语义、同一 pwsh）：2B 时 18/18 `exit 0xC000013A` 且含 `totalWidth`；本轮 18/18 存活。两者之间发生的唯一环境变化是**本机重启**（另有：2B 期间同时存在多个残留 app.exe/ConPTY 实例）。
- 待验证假设（中低置信度）：崩溃依赖会话级 conhost/ConPTY 状态（例如长时间运行后累积的控制台状态），而非 flags/尺寸/PSReadLine 配置；可复现路径待找（建议：连续启动大量 ConPTY 会话后再试）。
- **未实现任何启动参数追加**：任务要求"找到变量才实现"，本轮未找到（崩溃不存在），故 windows.rs spawn 参数未改。

## 5. 启动耗时（release，各 5 次）

| 配置 | 首帧中位数 | 极差 | 后台字体到位中位数 |
|---|---|---|---|
| 默认（无 --config，cmd.exe，内置字体） | 283 ms | 228 ms | 413 ms |
| clip-win.toml（cmd + Microsoft YaHei Mono 主字体 + YaHei 回退） | 1790 ms | 171 ms | 2383 ms |
| consolas-win.toml（cmd + Consolas 主字体 + YaHei 回退） | 1465 ms | 92 ms | 2032 ms |

- 原始序列：默认 (467,284,263,239,283)；YaHei Mono (1867,1824,1790,1725,1696)；Consolas (1468,1465,1395,1468,1376)。
- 工程判断（数据支持）：把 Consolas 作为主字体、CJK 走 YaHei 回退，比直接用 YaHei Mono 做主字体首帧快约 325 ms（-18%）、后台字体到位快约 350 ms，且极差更小（92 vs 171 ms）。给用户的配置建议即此组合。
- 2B 时同口径为 319 ms / 2071 ms（默认 / YaHei Mono），本轮更好，差异原因未定位（待验证假设：缓存温度）。

## 6. 未解决问题

1. `cargo test` 的 `path_priority_cli_env_default` 在 Windows 仍失败（实际断言见 §1），与 2B 相同，Linux 的修复只覆盖了 temp 目录构造。
2. pwsh FailFast 的触发条件仍未定位（本轮不复现），属**已知限制/待验证**，不作为回归项。
3. c5 中 5 个 `MARK` 里有 1 个被并发粘贴的数据切断（字节总数与 M/R/K 字符数均正确），说明键入与粘贴在 PTY 写侧是交错的、字符不保证连续；不构成丢字符。
4. 2A case a（cmd echo）在首跑出现一次焦点竞态失败，重跑通过；同类竞态在 2B 的 m2 也有，e2e 已用"预热点击 + 重试"缓解。

## 7. 对共享代码的请求（已写入 docs/WIN-NOTES.md）

1. `crates/config/tests/config.rs` 的 `path_priority_cli_env_default`：XDG_CONFIG_HOME 断言在 Windows 不成立（`default_config_path()` 用 APPDATA）；请按平台分支或跳过该断言。
2. 观察项（非阻塞）：ConPTY 会把控制台宽度处的折行物化为 CRLF，因此 2C.1 的"软折行拼接"在 Windows 上无法经真实 shell 触发；功能本身在 vt 层已验证（probe_wrap：wrapped=true）。若希望 Windows 也能验证，需要一处不经过 conhost 折行的输入路径。
3. 2B 的请求 6（1MB 粘贴排空）已在 2D.2 修复并经本机验证（静止 1048576 精确），该请求可关闭。

## 8. 交付物与证据

- 新增脚本：`scripts/windows/e2e/run_2c_win.ps1`（2C 复验 8 例）、`run_2d_perf.ps1`（三配置耗时）、`cfg/{title-fixed,pos60,consolas-win,pwsh-win}.toml`、`vt-verify-win/{osc123,osc999,osc123hold,osc999hold,wrap200}.ps1`、`vt-verify-win/{probe_wrap,probe_paste,probe_ptyin,probe_pwsh}` 相关改动。
- 日志：`vt-verify-win/logs/{2c-win-8.log,2d-runall-1/2.log,2d-clip-2.log,2d-mouse-1.log,2d-perf-1.log,2d-test.log,2d-clippy.log,wrap200.txt}`。
- 用例产物：`vt-verify-win/logs/phase2b-win/{c1..c5}*.{dump,err,png}`、`c5-probe.txt`。

## 9. 推送到位与核对

- 本地提交：33287ab `2D-win: merge phase2d-done, 2C re-verification, pwsh matrix, startup timings`（150 文件，+3446/-147）。
- `git push origin win`：`3811f91..33287ab  win -> win`。
- `git rev-parse win` = 33287abc5c796d5f8ac7c43e775f6bd417b36bc1；`git ls-remote origin refs/heads/win` = 33287abc5c796d5f8ac7c43e775f6bd417b36bc1（一致）。

## 10. 手动清单（≤3）

1. 用真实输入法（微软拼音）在 app 内输入中文，确认候选窗位置与上屏（本机无输入法注入手段，c3 只验证了预编辑叠加渲染）。
2. 用 Windows Terminal 或 Alacritty（本机未安装）跑一次 pwsh 7，确认 §4 的崩溃是否只在 portable-pty ConPTY 出现。
3. 若关心启动速度，把 `font.normal` 换成 Consolas、`fallback` 留 Microsoft YaHei，体感首帧差异（本机数据见 §5）。
