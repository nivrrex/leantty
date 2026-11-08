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
