# PINS — vt-verify 构建固定版本

来源均为实际查证，链接附后。状态标注：已验证事实 / 工程判断 / 待验证假设。

| 项目 | 值 | 来源 | 状态 |
|---|---|---|---|
| 绑定库 | Uzaaft/libghostty-rs @ 84373f8d48b8f0b794e259f74a71a52686616f5d | `git clone https://github.com/Uzaaft/libghostty-rs.git` 后 `git rev-parse HEAD` | 已验证事实 |
| 绑定库版本 | libghostty-vt / libghostty-vt-sys 0.2.1，MSRV Rust 1.96 | vendor/libghostty-rs/Cargo.toml `[workspace.package]` | 已验证事实 |
| Pinned Ghostty commit | 0b8b9984ee560456194262d190aa850224edb1f4 | vendor/libghostty-rs/crates/libghostty-vt-sys/build.rs `GHOSTTY_COMMIT` | 已验证事实 |
| Ghostty 版本 | 1.4.1-dev（pinned commit 处 build.zig.zon） | https://raw.githubusercontent.com/ghostty-org/ghostty/0b8b9984ee560456194262d190aa850224edb1f4/build.zig.zon | 已验证事实 |
| Ghostty 最低 Zig | 0.17.0（minimum_zig_version） | 同上 build.zig.zon | 已验证事实 |
| 绑定库要求的 Zig | 0.17.x（"Requires Zig 0.17.x on PATH, or selected through ZIG"） | vendor/libghostty-rs/README.md "Building" 一节 | 已验证事实 |
| 实际安装 Zig | 0.17.1 | https://ziglang.org/download/index.json （0.17.x 最新 release） | 已验证事实 |
| Zig 下载链接 | https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.1.tar.xz | ziglang.org/download/index.json | 已验证事实 |
| Zig 校验 | sha256 3581863e3cbcfd0fce0efc682ce7e9e93da96668a977791d356a26d2a7e9b17a | index.json `shasum` 字段，实测 `sha256sum -c` 通过 | 已验证事实 |
| Zig 安装位置 | ./vt-verify/toolchain/zig（未装进系统目录） | 本任务约束 | 已验证事实 |

注：build.rs 本身不断言 Zig 版本（仅注释提及 "Zig 0.17 removed --global-cache-dir"），0.17.x 要求来自绑定库 README 与 Ghostty 源码的 minimum_zig_version，两处一致。

## 构建期依赖处理（已验证事实）

- Ghostty pinned commit 的 translate-c 包（translate_c-2.0.0）依赖 aro：
  `git+https://codeberg.org/ziglang/arocc#d0c8c4d9c55daa7ef6e40cf0f630a5b5e900989b`，
  来源：vendor/ghostty-src/pkg/translate-c/build.zig.zon 与 ghostty 根 build.zig.zon.txt。
- 实测 codeberg.org 整体不可达（HTTP 503 / 连接超时，2026-11-07）。
- 合法解决（未修改上游代码）：同 commit 的 GitHub 镜像 Vexu/arocc
  （https://github.com/Vexu/arocc，API 确认含 commit d0c8c4d9...），
  `zig fetch git+https://github.com/Vexu/arocc#d0c8c4d...` 得到的包 hash
  与 ghostty zon pin 的 `aro-0.0.0-JSD1QtuBNwCASyBtNF3pqTl_W3oAJQGEVyFAtrBSE_Pa`
  一致（内容 hash 由 git commit 确定），zig 构建即命中缓存。
- 来源链接：https://codeberg.org/ziglang/arocc （503 未核实内容）、
  https://github.com/Vexu/arocc （已验证含同 commit）。

## GHOSTTY_ZIG_SYSTEM_DIR 目录构造方法（已验证事实）

- 布局依据：ghostty 官方 nix 打包 nix/build.zig.zon.nix（仓库根 build.zig.zon.nix）：
  `--system` 目录直接含 `<zig包hash>/` 子目录（tar.gz 解压后内容，无 p/ 层）。
- 构造：`~/.cache/zig/p/*.tar.gz` 解压至 zig-system/；p/ 下已解压的 N-V-*/目录直接复制入。
- 离线语义：`zig build --system <dir>` 等价于 "Disable package fetching"（zig build --help）。
- 网络不可达验证：http_proxy/https_proxy 指向本地不可达地址 127.0.0.1:9，zig build 与 cargo build --offline 均成功。
