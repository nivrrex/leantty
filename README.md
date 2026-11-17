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
- **代价**（同机对照，**直接句柄重定向**、n=10）：启动 `first_frame` 中位 419ms（系统 conhost）
  → 524ms（官方 ConPTY），即 **约 +105ms**（`PHASE3M-WIN.md` §4.1）；2M 行吞吐 330s → 355s
  （**约慢 7%**）。关闭耗时、粘贴精确送达、`take_writer` 停顿（86ms < 100ms）无劣化
  （3K §3.5/§3.8/§3.9）。
- **关于旧的"首帧 1.6–1.9 秒 / +110ms"绝对数字**：3K 及更早的 Windows 启动数据是用 PowerShell
  `Start-Process -RedirectStandardError` 采集的，该方式下每次写 stderr 约 6.8ms，会把首帧绝对值
  抬高约 4 倍（同机 bash 直接重定向下 419ms 对 1772ms），**只能横向比较、不能当作真实启动耗时**
  （`PHASE3L-WIN.md` §2.1 与 `PHASE3M-WIN.md` §4.2）。
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
| 打印 GPU 诊断（适配器清单/选中项/backend/surface 格式与 present mode） | `LEANTTY_PERF_FRAMES=1` |
| 指定后端（如 `dx12`、`vulkan`） | `WGPU_BACKEND`（未设置时 Linux=Vulkan，Windows=Vulkan+DX12） |
| 指定适配器（名字子串，不区分大小写） | `WGPU_ADAPTER_NAME`（未匹配会给出可用列表并报错） |

选适配器时先排除软件实现（`DeviceType::Cpu`，含 Windows 的 WARP/Basic Render Driver），
再按 Discrete > Integrated > 其他 的类型优先级选择（同级保持枚举顺序）。

**已实测（2026-11-15，一次真实 GPU 机器）**：AMD Ryzen 5 5600U 核显，Vulkan 与 DX12 均可见，
选中 Vulkan 适配器；GPU init 140–191ms；GPU 与 CPU 在共有文字行上**逐像素最大差 1 LSB**；
首帧 GPU 325–356ms、CPU 359ms；浸泡稳定（无 panic/无泄漏）。**DX12 尚未在真实硬件上验证**，
仅在 WARP（DX12 的软件适配器）上验证过。Windows 的 `backend = "auto"` **恒为 CPU**，`gpu` 是显式
实验开关（见上）。

**软件适配器诊断开关**（阶段 4Q）：`LEANTTY_GPU_ALLOW_SOFTWARE=1` 允许选中软件 GPU 适配器
（Linux 的 lavapipe、Windows 的 WARP / Basic Render Driver），**仅当 `backend = "gpu"` 时生效**；
`backend = "auto"` 永远忽略该变量。选中软件适配器时会默认打印一行用户可见警告
（"正在使用软件 GPU 适配器：<名称>，仅用于诊断"），诊断日志里标 `software=true`。
排序规则不变（Discrete > Integrated > 其他），所以软件适配器只有在没有真实适配器时才会被选中
（除非同时用 `WGPU_ADAPTER_NAME` 指定它）。

**已知未验证项**（截至阶段 4P）：真实 Windows GPU 上的 present 行为；DX12 与 Vulkan 在
Windows 上的差异；无 GPU 的 Windows 机器上 `backend="gpu"` 的实际退出路径（Linux 端已用
"无 ICD" 场景覆盖）。

## 诊断输出（默认关闭）

启动与运行期的 `[perf] …` 性能/诊断行**默认不打印**（每次启动 30+ 行，且在小体积重定向
宿主下单行写入可达毫秒级）。需要时设置已有环境变量 `LEANTTY_PERF_FRAMES=1` 即恢复，例如：

```sh
LEANTTY_PERF_FRAMES=1 ./target/release/leantty 2>perf.log
```

- **采集 `[perf]` 日志请用直接句柄重定向**（如 `2>perf.log`）。不要用 PowerShell 的
  `Start-Process -RedirectStandardError`：实测该方式每次写约 6.8ms，会把首帧抬高约 4 倍并发
  （`PHASE3L-WIN.md` §2.1）。
- 打开后会打印启动分段（`main_ms`/`config_ms`/`event_loop_ms`/`fontdb_ms`/`font_steps_us`/
  `first_frame_ms` 等）与每 50 帧的耗时统计。
- **面向用户的警告与错误不受影响**（配置告警、`hinting` 不支持、backend 回落、resize 说明等
  始终输出）。
- **`LEANTTY_FORCE_LIVENESS_POLL=1`**（release 可用）：让非 Windows 平台也走"存活检查唤醒"
  分支（Windows 恒开启该分支，因为 ConPTY 输出管道不会因 shell 退出而 EOF，退出检测只能靠
  轮询 `try_wait`）。仅用于在 Linux 上复现/验证 Windows 的"子进程退出后窗口不关闭"问题，
  会带来每秒一次的空闲唤醒，**不用于日常运行**（`PHASE4V-LINUX.md` §2）。
- 其它诊断钩子（`LEANTTY_DUMP_SNAPSHOT`、`LEANTTY_FRAME_HASH`、`LEANTTY_TEST_PASTE` 等）仅 debug 构建存在。
- **wgpu 自身的 `WGPU_*` 环境变量在本构建中不生效**：本项目以 `default-features = false` 引入
  wgpu 30.0.2，未启用其 `std` 特性，而 wgpu-types 的 `env::var` 在无 `std` 时恒返回 `None`
  （`wgpu-types-30.0.2/src/env.rs`），因此 `WGPU_BACKEND`（wgpu 侧）、`WGPU_POWER_PREF`、
  `WGPU_DX12_*`、`WGPU_GLES_MINOR_VERSION` 等一律无效。
  本项目**只识别以下由自身解析的变量**：`WGPU_BACKEND`（本项目自己解析，逗号列表，大小写不敏感、
  允许空白，无法识别的记号会报错）、`WGPU_ADAPTER_NAME`（名字子串匹配）、
  `LEANTTY_GPU_ALLOW_SOFTWARE`（允许软件适配器，仅 `backend="gpu"` 生效）、`LEANTTY_PERF_FRAMES`（诊断日志开关）、
  `LEANTTY_FORCE_LIVENESS_POLL`（诊断：非 Windows 也启用存活轮询）。

## 子进程启动（`terminal.shell`）

- **`terminal.shell.program`**：显式配置优先；未配置时依次回落到环境变量 `SHELL`、平台默认
  （Unix 为 `/bin/sh`，Windows 为 `cmd.exe`）。见 `crates/app/src/lib.rs::default_shell`。
- **`terminal.shell.args`**：参数按 TOML 数组解析；**非数组类型**会告警并整条忽略
  （`terminal.shell.args: expected array, ignored`），数组里的**非字符串元素被静默丢弃**
  （`crates/config/src/lib.rs` 的 `filter_map`）。数据结构区分三态：**未配置 = `None`**、
  **显式空数组 = `Some([])`**、配置了具体参数 = `Some([...])`（`ShellConfig.args:
  Option<Vec<String>>`）。
  - **Linux 与 Windows 都已生效**：`Pty::spawn_with_args` 把 `Some([...])` 逐项作为子进程参数
    （Unix 在 `crates/pty/src/unix.rs`，Windows 在 `crates/pty/src/windows.rs`）。
  - **用户显式配置优先**：只要写了 `args`（含显式空数组 `args = []`），就**原样使用、不注入任何
    默认参数**。
  - **Windows 未配置 `args` 时的默认值**（3U 偏离，已批准）：`cmd.exe` → `/K "chcp 65001"`；
    `powershell.exe` → `-NoLogo -NoExit -Command chcp 65001`；**`pwsh` 与其它 shell → 无参数**。
    `pwsh` 在旧实现里也不匹配 `powershell.exe` 这一精确文件名（本来就没有默认参数，谈不上"被取消"），
    它对中文输出的影响**未核实**；Windows 上建议显式写 `args` 自行控制代码页。
  - **Unix 未配置 `args` 时的默认值**：无（`/bin/sh`、`bash` 等不需要代码页参数）。
  - Linux 侧该字段过去被忽略、现在生效：**不配置 `args` 时行为与过去完全一致**
    （`spawn` 委托 `spawn_with_args(shell, None, …)`，字节序列不变），只有显式写了 `args`
    才会改变启动参数。

## 已知限制

以下每条都来自本仓库的实测或 Windows 侧报告，并标注出处；无出处的一律不写。

### 平台与部署

- **DX12 后端未在真实硬件验证**：Windows 的 `gpu` 后端（Vulkan/DX12）只在 WARP（软件适配器）下
  跑通，Linux 侧只验证到编译与链接成功（依据：`PHASE4P-LINUX.md` §2、`PHASE4X-LINUX.md` §5）。
- **Windows 发布包（RC1/RC2）未在干净机器上跑过**：只在开发机验证；RC2 的 zip 为 6607474 B、
  sha256 `868ce25430cc12646282de5a15715a4b5d01c31972e150be8615159828149f2f`，该数值由 Windows 侧
  自述，Linux 侧**未核实**（依据：`PHASE3U-WIN.md`、`PHASE4X-LINUX.md` §5）。
- **Windows 侧部分交互用例"未测"**：Windows 上自动输入注入（等价于 `xdotool type` 的手段）不可靠，
  交互类用例只能人工验证或跳过；这些项在 Windows 报告里标注"未测"，不等同于通过
  （依据：`PHASE3U-WIN.md`）。
- **Windows 的 `auto` 后端恒为 CPU**：要用 GPU 必须显式 `backend = "gpu"`（实验性）
  （依据：`PHASE4R-LINUX.md` §5）。
- **本项目自身只识别少数环境变量**：`WGPU_BACKEND`（自解析）、`WGPU_ADAPTER_NAME`、
  `LEANTTY_GPU_ALLOW_SOFTWARE`、`LEANTTY_PERF_FRAMES`，另有诊断用 `LEANTTY_FORCE_LIVENESS_POLL`；
  wgpu 自身的 `WGPU_*` 因 `default-features = false`（无 `std` 特性）全部不生效
  （依据：`PHASE4R-LINUX.md` §3）。
- **Windows 系统 conhost（19044）的既有缺陷**：物化 `ESC[7m`（变成 `ESC[30m`+`ESC[47m`）、
  `OSC 104` 在两种 ConPTY 下都不到达、输入通道吞掉括号粘贴标记与 SGR 鼠标序列、在控制台宽度处
  把折行物化为 CRLF（依据：`PHASE3G-WIN.md` §4.1/§4.2、`docs/WIN-NOTES.md`）。随附官方 ConPTY
  后可改善，`OSC 104` 除外（两种 ConPTY 下均不到达，见 `PHASE3K-WIN.md` §3.3）。
- **Windows 上"粘贴大文本后 Ctrl-C"无法立即恢复输入**：app 侧丢弃尚未写出的粘贴数据，但已进入
  控制台输入缓冲/PSReadLine 行内的部分无法撤回；5MB 粘贴 0/5 通过、100KB 0/2
  （依据：`PHASE3I-WIN.md` §4）。Linux 无此限制。
- **Windows 上子进程退出依赖 1 秒一次的低频存活检查**：ConPTY 输出管道不会因 shell 退出而 EOF
  （依据：`PHASE4T-LINUX.md` §5、`PHASE4V-LINUX.md`）。

### 渲染

- **Windows 上 CPU 与 WARP 约 1% 的截图差异主要在字形光栅**，不在滚动条列：960 宽实测差异
  1.0304%，滚动条列仅 9 px（0.156%）、列外字形区 4739 px（≈1.03%）；Linux 上 NVIDIA 与 lavapipe
  该列差异为 0，未复现同类差异（依据：`PHASE3T-WIN.md` t5、`PHASE4W-LINUX.md` §5；4S §3 的
  "1.46% ≈ 滚动条列 1.5%"归因已被推翻）。
- **CPU 后端在有事件时仍走整帧"快照 + 比对"**：空闲已完全静默，但事件驱动的每一帧仍要构快照
  （GPU 后端另有空闲跳过作第二道防线）（依据：`PHASE4S-LINUX.md` §7）。
- **`[perf] loop_gap_ms` 含"空闲等待"**：4S 之后空闲不再被定时唤醒，长间隔可能只是等待而非停顿，
  判停对应结合同窗口 CPU tick（依据：`PHASE4S-LINUX.md` §2.6）。

### 交互与输入

- **`Ctrl-C` 取消粘贴后，已经写进 PTY 的字节仍会被 shell 执行**：撤销只能做到"丢弃未写出的部分"
  （依据：`PHASE4H-LINUX.md` §3、`PHASE4L-LINUX.md`）。
- **粘贴积压超过 12MiB 时整条拒绝**（一个字节都不写，日志 `[perf] paste_rejected … oversized=`）
  （依据：`PHASE4L-LINUX.md` §2/§3）。
- **7MB 级粘贴在"回显开启"的 shell 里存在回显背压**：实测只送达 4.19MB（关闭回显后 7,280,000
  字节精确送达）（依据：`PHASE4H-LINUX.md` §3.5、e2e `case39`）。

### 未定位的异常（已知，未解决）

- **CPU 后端 30 分钟 soak 在 21.6 分钟提前退出**：无 panic，退出紧接 Ctrl-C 取消粘贴（子 shell
  随之退出，app 按设计关闭）；8 轮定向复现未重现，机制**未定位**（依据：`PHASE4U-LINUX.md` §4.2）。
- **GPU 后端自动滚动偶发 605 行**（同配置其余两次 1094/1099 行，≈369 行/秒）：未定位；推翻条件是
  稳定出现 ≥25% 差距（依据：`PHASE4U-LINUX.md` §2.3）。
