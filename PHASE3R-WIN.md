# PHASE3R-WIN：Windows 侧 4S 交接复测（空闲静默 / 滚动条列对拍 / 截断日志）

- 日期：2026-11-16（按系统时间）
- 范围：只做 4S 交接清单（`PHASE4S-LINUX.md` §6.1、§6.2、§6.3）在 Windows 上的复测；**未改 `crates/`**，只新增验证脚本与日志（`vt-verify-win/`）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- 环境事实：仓库已从 `C:\dev\tty` 移到 `E:\dev\tty`（C 盘满）；本机无真实 GPU，仅 DX12 WARP（`Microsoft Basic Render Driver`）。

## 0. 进程处理

- 只结束本任务自己启动、按 PID 核对的 `app.exe`（`Stop-Process -Id`），未用通配名称清理。
- 未结束任何 `OpenConsole.exe` / `conhost.exe` / `pwsh.exe`。

## 1. §6.1 空闲静默（已验证事实）

口径按 4S §6.1：输出停止收尾后 12 秒内不得出现新的 5 秒统计行，且 CPU 时间增量≈0。

| 运行 | 配置 | 观察窗 | CPU 增量 | 5 秒统计行 | loops / wake_timer / wake_pty |
|---|---|---|---|---|---|
| 空闲（`bash -l` 只到提示符） | `tonight-kit-v4/configs/cpu.toml` | t=5s→18s（13s） | **0.000 s**（1.171875→1.171875） | **1**（仅启动首行 `loops=1 wake_event=1`） | 1 / 0 / 0 |
| 对照（持续输出 `echo tick`，0.2s 间隔） | 同上 | t=5s→13s（8s） | 0.03 s | **2**（每 5s 一条） | 17 / 14 / 3 |

- 对照证明该指标对"循环被唤醒"敏感：有输出时统计行按 5 秒节奏出现、`loops` 明显增长；空闲时 `about_to_wait` 不再被定时唤醒（4S t2 的 `ControlFlow::Wait` 修复在 Windows 上同样生效）。
- 判定：**通过**（cpu 后端；本机无 GPU，`gpu` 后端空闲帧未测，见 §4）。
- 证据：`vt-verify-win/idle_test.ps1`、`idle_ctl.ps1`、`logs/idle-stats.log`、`logs/idle-ctl.log`。

## 2. §6.2 滚动条列对拍（已验证事实，仅 WARP）

4S §3 的解释是"3Q 观察到的 1.46% 像素差 ≈ 滚动条列占画面宽度 1.5%"。在 Windows 上用
`crates/render/tests/gpu_parity.rs` 的两个滚动条用例（`rows_sb=2000`，视口底部/中段）复测：

| 适配器 | 视口 | 列内差异 | 最大通道差 | 列外差异 | 列内像素占全图 |
|---|---|---|---|---|---|
| DX12 WARP（`Microsoft Basic Render Driver`，type=Cpu） | 底部 | **0 / 5472（0.000%）** | 0 | 0.000% | 1.500% |
| 同上 | 中段 | **0 / 5472（0.000%）** | 0 | 0.000% | 1.500% |

全量 `gpu_parity`（10 例）：**10 passed / 0 failed**（28.3s），`glyph_max=1`，
背景区差异最大 `23/8 770 475`（0.000%），字形区差异占比 1.93%~2.47%（4A 上限 8.3%）。

- 结论（工程判断，置信度高）：WARP 上滚动条列差异为 0、列内像素占全图 1.500%，与 3Q 的
  1.46% 相符；4S §3 的归因在 Windows 软件适配器上成立。
- 未覆盖：真实 GPU（NVIDIA 类）在 Windows 上未测——本机仅 WARP。
- 证据：`logs/gpu-probe-gpu.toml.log`（适配器枚举 + 软件适配器提示）、
  `cargo test --release -p render --test gpu_parity -- --nocapture` 输出。

## 3. §6.3 scrollback 截断日志限速（部分完成）

- 本轮多轮 `app.exe` 运行（CPU 空闲、持续输出、GPU/WARP、`seq 1 50000` 压力）的 stderr 中
  `[selection] scrollback 截断` **0 条**：未观察到刷屏（正面证据有限，见下）。
- 构造选区需要 GUI 注入，本机不可用：`SetForegroundWindow` 受前台锁失败（ALT 点击绕过后
  仍失败）；`keybd_event`/`mouse_event`（已按 winit 要求补 scan code）、`PostMessage WM_CHAR`
  三条路径均未送达——PTY 唤醒计数 `wake_pty` 无增长、`APP_DUMP_SNAPSHOT` 中无输入回显。
  4 轮尝试后停手（未继续猜测注入方式）。
- 限速分支本身平台无关（`crates/app/src/lib.rs` 的 `last_sel_clamp_log`，1 秒一条），4S 已在
  Linux soak 实测 490 行/秒 → ≤1/秒。Windows 实机频率：**未测（无输入通道）**。

## 4. 未解决问题

1. **真实 GPU 未测**：本机仅 DX12 WARP；§6.2 的"真实 GPU"要求仍需在 NVIDIA/Intel 机器上跑同一用例。
2. **§6.3 Windows 实机频率未取得**：GUI 输入注入在本机不可用（前台锁 + 键鼠消息均未送达），
   只取得"未观察到刷屏"的间接结果。
3. **GPU 后端空闲渲染未复测**：4Q 的 GPU 空闲跳过只在 Linux 验证；WARP 上可用本报告 §1 同法
   加测（未做，超出 4S 交接的 3 项）。
4. **验证脚本存在硬编码旧路径**：`scripts/windows/e2e/wb2_lib.ps1` 的 `$ROOT = 'C:\dev\tty'`
   仍指旧位置（仓库移动后失效），本轮未改（不在 3R 范围），已知问题。

## 5. 复现命令

```
# 构建/测试（仓库移动后需显式给三件套；CARGO_TARGET_DIR 用仓库内 target 以复用产物）
cd E:\dev\tty
set ZIG_REAL=C:\vt\toolchain\zig\zig.exe
set ZIG=E:\dev\tty\scripts\windows\zig-inject-target.cmd
set GHOSTTY_SOURCE_DIR=C:\vt\vendor\ghostty-src
set GHOSTTY_ZIG_SYSTEM_DIR=C:\vt\zig-system
set CARGO_TARGET_DIR=E:\dev\tty\target
set APP_GPU_ALLOW_SOFTWARE=1
cargo test --release -p render --test gpu_parity -- --nocapture

# 空闲静默
powershell -NoProfile -ExecutionPolicy Bypass -File E:\dev\tty\vt-verify-win\idle_test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File E:\dev\tty\vt-verify-win\idle_ctl.ps1
```

## 6. 手动清单（≤3）

1. 有真实 GPU 的 Windows 机器上跑 `gpu_parity` 的滚动条两例，确认列内/列外差异与 WARP 一致。
2. 交互式桌面下手动拖选 + 大量输出，观察 `[selection] scrollback 截断` 是否 ≤1 条/秒。
3. WARP 上按 §1 同法测 `gpu.toml` 的空闲静默（本报告只测了 cpu 后端）。
