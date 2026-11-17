# 项目总结（阶段 0 → 4X）

只写结论：决策与否决项、已验证事实、已知限制、挂起项。过程细节见各阶段报告
（`PHASE1-LINUX.md` … `PHASE4X-LINUX.md`、`PHASE*WIN.md`、`docs/DECISIONS.md`）。

## 1. 项目做了什么

一个自研终端：VT 解析与网格用 libghostty-vt（vendored），渲染给两套后端（CPU 软光栅 /
wgpu-Vulkan；Windows 上加 DX12 实验支持），PTY 用 portable-pty，窗口/输入用 winit，
配置用 TOML。仓库 `crates/{vt,render,config,pty,app}` + `gpu-spike`（选型探针）+ e2e 脚本。

## 2. 关键决策与被否决项

| 决策 | 否决项与原因 |
|---|---|
| VT/网格用 libghostty-vt（vendored，离线构建，zig 侧自包含）| 自研 VT 解析：宽字符/组合/重排语义成本过高 |
| 渲染用 fontdue 光栅 + 自建字形缓存与 atlas | swash/其它光栅库：4B 已定型，迁移收益不明 |
| GPU 用 wgpu `=30.0.2`（Vulkan，`default-features=false`）| 自写 Vulkan/DXGI：维护成本高；启用 wgpu `std` 特性：会拉入其环境变量语义且体积增大 |
| 反色/默认色决策集中在 `render/src/decide.rs` | 各处就地判断：4C 实测出现过不一致 |
| present 只提交脏区；不做滚动位图复用 | 复用：3B 实测收益为负（release 下 present 仅占 10–16%）|
| 粘贴积压整条拒绝（上限 12MiB）| 截断尾部：会丢 `ESC[201~` 并切开 UTF-8（4L 实测）|
| 大粘贴走独立写线程（`PtyWriter`，1MiB 队列 + 2s join）| 主线程直写：4H 实测 12.9s 停顿 |
| 输入统一"先回底"（`follow.rs` 纯决策）| 各处零散判断：4K 证明能力从未实现 |
| 字体共享用 `Arc<Font>` | 每面克隆：4N 实测 Noto CJK 每面 298.7MB、4 组各一份 |
| 空闲用 `ControlFlow::Wait`（入口重置，有代办才升级 `WaitUntil`）| 靠空转顺带驱动：4S 实测 249 loops/s、约 13% 单核 |
| 每帧维护（OSC 标题、拖选自动滚动）前移到渲染之前 | 留在渲染后：被 GPU 空闲跳过/CPU 空 damage 的提前 return 吞掉（4T）|
| 存活唤醒收敛到唯一出口 `finish_wake` | 只在函数末尾登记：7 处提前 return 全跳过（4V，Windows 3S 现象）|
| `terminal.shell.args` 三态（未配置/空数组/具体值），显式配置优先 | 无条件注入 chcp 默认值：3N/3T 的覆盖问题 |
| 3U 偏离（已批准）：Windows 未配置 args 时仅 `cmd.exe`/`powershell.exe` 给 chcp 默认值 | 对 `pwsh` 注入默认参数：`pwsh` 与 `powershell.exe` 是不同程序，行为差异大 |

## 3. 已验证事实（可直接依赖）

- **性能**：release 下 present 占 10–16%；首帧 ~184ms（`Arc<Font>` 后）；GPU 与 CPU 逐像素
  1 LSB 内一致；4S 起两后端空闲 loops=0/timer=0/ctx=0。
- **渲染正确性**：`frame_bytes_identical` 黄金哈希 `507485fb7945fb8f`（字体指纹
  `DejaVuSansMono.ttf:343140:c69b:8x16`，指纹不符时 SKIP 并在输出中标注）；`gpu_parity` 10/10；
  宽字符裁剪、字形基线、反色语义、调色板到达 VT 均有像素级用例。
- **内存**：字体共享后 RSS nofb 88MB / simhei +129MB / Noto JP +311MB（此前 209/+637/+1508MB）。
- **输入**：10MB 粘贴精确送达（关闭回显）；12MiB 上限、超限整条拒绝且不动视口；Ctrl-C 丢弃
  未写出的粘贴并补发 `ESC[201~`；输入回底四档全 1.000。
- **门禁**：`scripts/gates.sh` 9 步（workspace 测试 / clippy debug+release / release 零告警 /
  e2e（含 GPU 轮）/ Windows 依赖树 wgpu==30.0.2 / windows-gnu check / wgpu 环境变量防护 /
  全仓 rustfmt）；tag 一律经 `scripts/tag_checked.sh`（要求门禁 hash 与 HEAD 一致）。
- **e2e**：113 项（含 GPU 轮 10 项），全部自动化；`:99` 上跑，X11 只经 `guard.sh` 的 `x11()`。

## 4. 已知限制

见 README「已知限制」一节（平台与部署 / 渲染 / 交互与输入 / 未定位的异常四组，逐条标出处）。
其中不确定项一律标注"未核实"：Windows RC2 的 sha256 数值与产物对应关系、`pwsh` 中文输出影响、
DX12 真实硬件表现、Windows 系统 conhost 的既有限制随 ConPTY 版本的变化。

## 5. 挂起项（未处理，原样列出）

1. `load_font_list` 的深拷贝（字体库每次列举都克隆字体数据）。
2. GPU 初始化前移（当前在首帧渲染路径内创建适配器/设备）。
3. 适配器选择的正式配置（现在只有 `WGPU_ADAPTER_NAME`/`LEANTTY_GPU_ALLOW_SOFTWARE` 两个临时开关，
   `WGPU_BACKEND` 由项目自解析）。
4. CPU 后端 30 分钟 soak 在 21.6 分钟提前退出（机制未定位，8 轮定向复现未重现）。
5. GPU 后端自动滚动偶发 605 行（同配置其余两次 1094/1099）。

## 6. 交付物

- Linux：`target/release/leantty`（自包含二进制，运行期仅依赖系统 X11/Vulkan 运行库）。
- Windows：RC2 zip（随附官方 ConPTY，需两文件同目录），由 Windows 侧产出与验证。
- 脚本：`scripts/gates.sh`、`scripts/tag_checked.sh`、`scripts/e2e/*`（e2e、soak、矩阵、探针）。
- 文档：各阶段 `PHASE*-LINUX.md` / `PHASE*-WIN.md`、`docs/DECISIONS.md`、本总结。
