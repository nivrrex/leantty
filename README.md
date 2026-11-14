# tty：自建精简终端

核心 VT 层基于 libghostty-vt（Uzaaft/libghostty-rs vendored，版本见 vt-verify/PINS.md）。

## 环境变量（构建时设置，仓库内不出现绝对路径）

- `ZIG`：zig 可执行文件路径（如 vt-verify/toolchain/zig/zig）
- `GHOSTTY_SOURCE_DIR`：ghostty 源码目录（vt-verify/vendor/ghostty-src，冻结副本）
- `GHOSTTY_ZIG_SYSTEM_DIR`：zig system 目录（vt-verify/zig-system，nix 布局）

未设置时脚本与构建会报清晰错误。

## 构建命令

```sh
export ZIG=<zig 路径>
export GHOSTTY_SOURCE_DIR=<ghostty 源码目录>
export GHOSTTY_ZIG_SYSTEM_DIR=<zig system 目录>
cargo build --workspace        # 调试构建
cargo test --workspace         # 测试
cargo clippy --workspace -- -D warnings
cargo build --release
```

## 结构

- `crates/vt`：唯一接触 libghostty-vt 的 crate；自有 `FrameSnapshot` 快照、resize/feed/on_pty_write/KeyEncoder/MouseEncoder 封装
- `crates/pty`：跨平台 PTY 抽象（unix 用 portable-pty；windows 占位）
- `crates/render`：`Renderer` trait + softbuffer/fontdb/fontdue 的 CpuRenderer
- `crates/app`：winit 事件循环接线
- `vendor/libghostty-rs`：vendored 绑定库
- `vt-verify/`：冻结的验证基线（V1-V8，勿改）

## 验证

```sh
sh vt-verify/checks/run_all.sh
```

## 已知平台限制

以下均为已在本仓库报告中实测证实的事实，不是推测：

- **Windows（系统 conhost，19044）会物化 `ESC[7m`**：到达 PTY 主端的是 `ESC[30m`+`ESC[47m`，因此"默认色反色块"表现为 normal.white 底而不是 primary.foreground；WezTerm conpty 则原样保留 `ESC[7m`（依据：`PHASE3G-WIN.md` §4.1/§4.2，`docs/WIN-NOTES.md` 请求 21）。
- **`OSC 104`（重置调色板）在 Windows 两种 ConPTY 下都不到达**：`OSC 4` 改色后无法用 `OSC 104` 复位（依据：`PHASE3G-WIN.md` §4.1 的 s4 行与 §4.2 第 4 条）。
- **Windows ConPTY 输入通道会吞掉括号粘贴标记与 SGR 鼠标序列**（`ESC[200~`/`ESC[201~` 只剩中间内容，`ESC[<0;10;5M` 到达 0 字节）；系统 conhost 还会把控制台宽度处的折行物化为 CRLF（依据：`docs/WIN-NOTES.md` 事实补充两条；`PHASE3D-WIN.md` 同类记录）。
- **GPU 后端仅 Linux**：wgpu 依赖只写在 `crates/render/Cargo.toml` 的 `[target.'cfg(target_os = "linux")'.dependencies]`，Windows 构建里 wgpu/pollster 命中为 0，恒用 CPU 渲染（依据：`PHASE4A-LINUX.md` 选型与每轮 tag 前的依赖树复核）。
