# PHASE1-LINUX：阶段1 Linux 实现交付

## 1. 完成项与测试结果（命令输出摘要）

- vt：FrameSnapshot 自有快照（不含 libghostty 类型）+ resize/feed/on_pty_write/KeyEncoder/MouseEncoder 封装。
  `cargo test -p vt` → 7 passed（V4 a-f 网格断言 6 项 + V8 owned 快照不变性 1 项）。
- pty：unix 实现（portable-pty），Pty trait 六方法；close 先释放主端再回收子进程；读线程 join 带 5s 超时。
  `cargo test -p pty` → 1 passed（echo 你好 入网格且汉字占 2 格、resize 双侧一致、close 不卡死）。
- render：CpuRenderer 基于 fontdb 查找 + fontdue 光栅化 + 自有帧缓冲；softbuffer surface 接入 app。
  `cargo test -p render` → 2 passed（合成快照背景格与有字符格像素不同；默认字体常量）。
- app：winit 0.30 ApplicationHandler 全接线（读线程 channel→feed、键盘→KeyEncoder→PTY、resize 双侧、on_pty_write→PTY、子进程退出→exit）+ bin 入口。
- 全量：
  - `cargo test --workspace` → 13 行 test result: ok（10 个测试 + 3 个空 doc-test）。
  - `cargo clippy --workspace --all-targets -- -D warnings` → 通过（0 错误 0 警告）。
  - `cargo build --release` → Finished release profile（4m00s）。
- 验证基线：`sh vt-verify/checks/run_all.sh` → 10/10 PASS（证据见 vt-verify/REPORT.md）。

## 2. 仓库状态

- main 最新 commit 与 skeleton-ready / verify-baseline tag 的 hash 及 git ls-remote 输出见本文件末尾"仓库状态实测"（交付提交后立即记录）。

## 3. 未解决问题、与本规格的偏差

- app 事件循环无运行级自动化测试（容器无显示器）；GUI 行为见第 5 节手动清单。
- 键盘映射为阶段1 子集（命名键 + ASCII 字母数字，其余忽略）。
- 网格像素尺寸固定 8x16（CELL_W/CELL_H），字体度量接入留待后续阶段。
- render 每格只光栅化首字符（等宽网格假设）。
- 规格"CpuRenderer 基于 softbuffer"的实现路径偏差：softbuffer 作为窗口 surface 后端接入 app，光栅化核心走 fontdue + 自有帧缓冲；像素断言针对帧缓冲而非 softbuffer surface。原因：合成快照测试无法驱动真实窗口 surface。已验证事实：像素断言通过；softbuffer 接线编译通过。
- 产物字节非确定性（继承自 vt-verify 基线已知问题，见 vt-verify/REPORT.md 未解决问题 1）。

## 4. 给 Windows 侧的交接（只记已验证的）

- crates/pty/src/windows.rs 为空占位（规格约束，本侧未改）；.cargo/config.toml 的 [target.x86_64-pc-windows-gnu] 为空段（规格约束）。
- Pty trait 契约（crates/pty/src/lib.rs 模块文档）：close() 先释放 PTY 主端再回收子进程；读线程 join 必须带超时、禁止无界 join。依据：Windows 侧 W5 实测数据（先等子进程退出会使主端 EOF 延迟约 83s）；该数据为 Windows 侧实测，Linux 侧未复现。
- vt crate 接口（FrameSnapshot / VtTerminal::feed/resize/snapshot/on_pty_write / KeyEncoder / MouseEncoder）跨平台一致；Linux 侧 10 项测试通过，Windows 直接复用。
- winit 0.30.13 的 run_app + ApplicationHandler 模式在 Linux 编译与 clippy 通过；Windows 编译未验证。
- 构建环境变量契约：ZIG / GHOSTTY_SOURCE_DIR / GHOSTTY_ZIG_SYSTEM_DIR（README），仓库内无绝对路径。
- pins：Zig 0.17.1、ghostty 0b8b9984、libghostty-rs 84373f8（vt-verify/PINS.md），不得升级。
- KeyEncoder 字节序列（V7）：普通/应用光标键模式 18 组断言在 Linux 通过（vt-verify/checks/v7_key.sh）；Windows 侧使用同一 vendored 库，预期一致。

## 5. 用户本机手动确认清单（自动测试已覆盖项不列入）

1. `cargo run --release -p app` 起 shell，键入 `echo 你好` → 窗口显示中文且汉字占 2 格宽。
2. 窗口内运行 vim → 全屏交替屏渲染，退出 vim 恢复主屏内容。
3. 拖拽窗口边缘调整大小 → 字符网格同步重排、无残影。
4. 关闭窗口 → 进程立即退出、无卡死（close 契约的运行级确认）。
5. 中文输入法组词上屏 → 文字完整入网格。
6. vim 内方向键（应用光标键模式）与 shell 内（普通模式）移动行为均正确。
7. Ctrl-C 终止前台进程、Ctrl-D 退出 shell。
8. F1/Home/End/Shift+Tab 按键 → shell/vim 响应与预期一致。

## 仓库状态实测

（见交付提交后 append 的 git ls-remote 与 git log 输出。）
