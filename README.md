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

## Windows：随附官方 ConPTY

Windows 默认使用系统 conhost（旧版 ConPTY）作为 pty 宿主；本项目**建议随附官方 ConPTY**
（NuGet 包 `Microsoft.Windows.Console.ConPTY`，版本 **1.25.261109001**，MIT 许可），
以获得更正确的序列透传。以下均为 `PHASE3K-WIN.md` 实测结论：

- **获取与校验**：用 `scripts/windows/conpty/fetch-conpty.ps1`（参数化 `-OutDir`/`-Proxy`）
  下载该固定版本，脚本会校验包 sha256、两个文件的 sha256、以及 Authenticode 签名必须
  `Valid` 且主体为 Microsoft；任一步失败即非零退出并清理临时目录（`PHASE3K-WIN.md` §1.3/§2）。
- **部署**：包内 `conpty.dll` 与 `OpenConsole.exe` 在**不同目录**，取出后必须与 `app.exe`
  **放在同一目录**（运行时按程序目录优先加载）（§1.3 备注、§2 的 README.md 条目）。
- **回退到系统 conhost**：删除或改名这两个文件即可，无需改配置（§2）。
- **对照结论**（S=系统 conhost、N=官方 1.25.261109001，§3.2/§3.3）：
  折行不再被物化为 CRLF（200 字符单行 S 为 `80/80/43` 分段并出现 CRLF，N 为单段 200 字符）；
  `ESC[7m` 原样到达（S 改写为 `ESC[30m`+`ESC[47m`）；`OSC 104` 可达（S 与 WezTerm 侧载均丢弃）。
- **代价**（同机对照，§3.6/§3.7）：启动 `first_frame` 中位 1673ms → 1787ms（**+约 110ms**）；
  2M 行吞吐 330s → 355s（**约慢 7%**）。关闭耗时、粘贴精确送达、`take_writer` 停顿
  （86ms < 100ms）无劣化（§3.5/§3.8/§3.9）。
- **启用门槛**：`PHASE3K-WIN.md` §3.10 的判定（a/d/e/h/i 无新增失败且 b 或 c 至少一项优于
  S）已满足，故采用。

## Windows GPU 后端（实验性）

Windows 目标现在也能编译并使用 wgpu 的 GPU 后端（Vulkan + DX12），但**只在配置显式写
`backend = "gpu"` 时启用**：

- `backend = "cpu"`：CPU 渲染（默认）。
- `backend = "auto"`：**Windows 上仍直接走 CPU**（不尝试 GPU）。Linux 的 `auto` 语义不变
  （先试 GPU，不可用回落 CPU）。
- `backend = "gpu"`：尝试 GPU；无可用适配器或初始化失败时**非零退出并打印错误**
  （与 Linux 一致，不再静默回退 CPU）。

诊断（全部用标准环境变量与已有开关，不新增配置项）：

| 用途 | 变量 |
|---|---|
| 打印 GPU 诊断（适配器清单/选中项/backend/surface 格式与 present mode） | `APP_PERF_FRAMES=1` |
| 指定后端（如 `dx12`、`vulkan`） | `WGPU_BACKEND`（未设置时 Linux=Vulkan，Windows=Vulkan+DX12） |
| 指定适配器（名字子串，不区分大小写） | `WGPU_ADAPTER_NAME`（未匹配会给出可用列表并报错） |

选适配器时先排除软件实现（`DeviceType::Cpu`，含 Windows 的 WARP/Basic Render Driver），
再按 Discrete > Integrated > 其他 的类型优先级选择（同级保持枚举顺序）。

**已知未验证项**（截至阶段 4P）：真实 Windows GPU 上的 present 行为；DX12 与 Vulkan 在
Windows 上的差异；无 GPU 的 Windows 机器上 `backend="gpu"` 的实际退出路径（Linux 端已用
"无 ICD" 场景覆盖）。

## 诊断输出（默认关闭）

启动与运行期的 `[perf] …` 性能/诊断行**默认不打印**（每次启动 30+ 行，且在小体积重定向
宿主下单行写入可达毫秒级）。需要时设置已有环境变量 `APP_PERF_FRAMES=1` 即恢复，例如：

```sh
APP_PERF_FRAMES=1 ./target/release/app 2>perf.log
```

- 打开后会打印启动分段（`main_ms`/`config_ms`/`event_loop_ms`/`fontdb_ms`/`font_steps_us`/
  `first_frame_ms` 等）与每 50 帧的耗时统计。
- **面向用户的警告与错误不受影响**（配置告警、`hinting` 不支持、backend 回落、resize 说明等
  始终输出）。
- 其它诊断钩子（`APP_DUMP_SNAPSHOT`、`APP_FRAME_HASH`、`APP_TEST_PASTE` 等）仅 debug 构建存在。

## 已知平台限制

以下均为已在本仓库报告中实测证实的事实，不是推测：

> 以下四条凡标注"使用系统 conhost 时仍适用"者，随附官方 ConPTY（见上一节）后可改善。
- **Windows（系统 conhost，19044）会物化 `ESC[7m`**：到达 PTY 主端的是 `ESC[30m`+`ESC[47m`，因此"默认色反色块"表现为 normal.white 底而不是 primary.foreground；WezTerm conpty 则原样保留 `ESC[7m`（依据：`PHASE3G-WIN.md` §4.1/§4.2，`docs/WIN-NOTES.md` 请求 21）。**使用系统 conhost 时仍适用**；随附官方 ConPTY 后 `ESC[7m` 原样到达（`PHASE3K-WIN.md` §3.3）。
- **`OSC 104`（重置调色板）在 Windows 两种 ConPTY 下都不到达**：`OSC 4` 改色后无法用 `OSC 104` 复位（依据：`PHASE3G-WIN.md` §4.1 的 s4 行与 §4.2 第 4 条）。**使用系统 conhost 时仍适用**；随附官方 ConPTY 后 `OSC 104` 可达（`PHASE3K-WIN.md` §3.3）。
- **Windows ConPTY 输入通道会吞掉括号粘贴标记与 SGR 鼠标序列**（`ESC[200~`/`ESC[201~` 只剩中间内容，`ESC[<0;10;5M` 到达 0 字节）；系统 conhost 还会把控制台宽度处的折行物化为 CRLF（依据：`docs/WIN-NOTES.md` 事实补充两条；`PHASE3D-WIN.md` 同类记录）。**使用系统 conhost 时仍适用**；随附官方 ConPTY 后折行不再物化为 CRLF（`PHASE3K-WIN.md` §3.2）。
- **GPU 后端仅 Linux**：wgpu 依赖只写在 `crates/render/Cargo.toml` 的 `[target.'cfg(target_os = "linux")'.dependencies]`，Windows 构建里 wgpu/pollster 命中为 0，恒用 CPU 渲染（依据：`PHASE4A-LINUX.md` 选型与每轮 tag 前的依赖树复核）。
- **Windows 上"粘贴大文本后 Ctrl-C"无法立即恢复输入**：app 侧会丢弃尚未写出的粘贴数据（3H 证据：5MB 粘贴中按 Ctrl-C 后子进程只收到 123130 字节），但**已进入控制台输入缓冲/PSReadLine 行内的部分无法撤回**，之后键入的命令会拼接在已送达文本之后——5MB 粘贴 0/5 通过、100KB 粘贴 0/2、单次 debug 等 90s 仍未恢复；对照"不粘贴直接输入同样命令"263ms 即生效（依据：`PHASE3I-WIN.md` §4）。Linux 无此限制（4H/4K 的 e2e 中 Ctrl-C 后提示符立即返回）。
