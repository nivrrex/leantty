# PHASE3K-WIN：随附官方新版 ConPTY 的评估与准备（附空闲内存与字体打开成本）

- 日期：2026-11-15（按系统时间）
- 范围：核实官方 ConPTY 再分发来源与许可；落盘随附脚本（不入库二进制）；三种 ConPTY 条件全量对照（S=系统 conhost、W=WezTerm 20240203 sideload、N=官方 1.25.261109001）；app 空闲内存基线；字体文件打开成本探针。未改共用代码、未加依赖、未改 trait。
- 环境：Windows 10 LTSC 19044.4529；Rust 1.99.1（x86_64-pc-windows-gnu）；portable-pty 0.8.1。测试期间用户自己的 WezTerm 与安全软件常驻（未结束任何不属于本任务的进程）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## 0. 过程事故（如实记录）

本轮早期我用 `Stop-Process -Id` 结束了两个 `OpenConsole.exe` 进程（PID 7552/11252），判断依据是"它们由我 sideload 的 conpty 产生"。**WezTerm 自身也使用 OpenConsole 作为 pty 宿主**，因此这两个进程有可能属于用户的 WezTerm 会话，随后用户报告终端会话中断、机器重启。此后本轮改为：**不结束任何 `OpenConsole.exe` 进程**（不论 PID 来源），暂存文件改为在目录层面清理；app 进程仍只结束自己启动并记录 PID 的实例。

## 1. t1 来源与许可（只取证）

### 1.1 网络路径（实测）

| 目标 | 结果 |
|---|---|
| `api.nuget.org` | 直连可用（302 → `nuget.azure.cn` 镜像，200） |
| `api.github.com` | 直连可用（200） |
| `raw.githubusercontent.com` | 直连失败（http=000）；**经 `http://127.0.0.1:7777` 代理可用（200）**；`http://0.0.0.0:7777` 失败 |

代理仅用于单条 curl 命令（`-x`），未改系统代理、未改全局 git 配置、未写入仓库文件；下载物全部落在 gitignore 的 `vt-verify-win/conpty-dl/`。

### 1.2 NuGet 包核实（`Microsoft.Windows.Console.ConPTY` 存在）

`api.nuget.org/v3/registration5-semver1/microsoft.windows.console.conpty/index.json` 共 1 页 11 个版本：

| 版本 | 发布日期 | license |
|---|---|---|
| 1.24.260303001 / 1.24.260512001 / 1.24.260710001 / 1.24.261001001 | 2026-03-05 … 2026-10-02 | MIT |
| 1.25.260303002-preview / 1.25.260427001-preview / 1.25.260512002-preview / 1.25.260710002-preview | 2026-03-05 … 2026-07-13 | MIT |
| 1.25.260930003 / **1.25.261109001**（稳定） | 2026-10-02 / **2026-11-11** | MIT |
| 1.26.260930004-preview / 1.26.261109003-preview | 2026-10-02 / 2026-11-11 | MIT |

- 选型：**1.25.261109001**（最新稳定版；1.26 只有 preview）。
- 注册索引未给出 `packageHash`/`packageHashAlgorithm`（两项均为 null），改以实际下载校验：包 sha256 = `3d77f535df848e44225c2a8912ac844fecb23f3057732b639f3034b587496f09`，sha512(base64) = `wA6Fbai2mUlsF2J+pmtWEoRTHiAisIGobFxJ9u4r9XFEug9JnOn0zucPLIb/ez93Jm4JUXr8pj+XAswWZpzLXg==`（flatcontainer 的 `.nupkg.sha512` 端点返回 404，故本地计算）。
- nuspec：`<license type="expression">MIT</license>`、`requireLicenseAcceptance=false`、`projectUrl=https://github.com/microsoft/terminal`、描述"allows applications to host Windows Console sessions… Windows 10.0.17763.0 and above"。

### 1.3 包内容与 x64 文件校验

包内（1.25.261109001，1 763 943 字节）含 `runtimes/win-x64/native/conpty.dll`、`build/native/runtimes/x64/OpenConsole.exe`、`inc/conpty.h`、`.signature.p7s`（NuGet 包签名）、各架构 lib；**两个文件在包内不同目录，部署时必须放在同一目录**。

| 文件 | 大小 | sha256 | FileVersion | ProductVersion | Authenticode |
|---|---|---|---|---|---|
| `conpty.dll` | 119 608 | `e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492` | 1.25.2611.09001 | 1.25.261109001 | **Valid**，`CN=Microsoft Corporation`，时间戳 Microsoft Time-Stamp Service |
| `OpenConsole.exe` | 1 089 336 | `b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775` | 1.25.2611.09001 | 1.25.261109001 | 同上 |

### 1.4 版本对照

| 来源 | 版本 | 备注 |
|---|---|---|
| 本任务选定（官方 NuGet） | **1.25.261109001**（文件版本 1.25.2611.09001） | 签名 Valid，MIT |
| WezTerm sideload（3D 用过） | 20240203-110809（conpty.dll 99 328 B / OpenConsole 1 395 712 B，无版本资源） | 2024-02 构建 |
| node-pty prebuild（3D 对照） | 1.23.2510.08001 | 2025-10 构建 |

### 1.5 许可（逐字保存于 `scripts/windows/conpty/LICENSE-ConPTY.txt`）

- 来源：`https://api.github.com/repos/microsoft/terminal/contents/LICENSE?ref=main`（sha256 `5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b`，1116 字节）；代理与 API 两条路径取到的文件 sha256 相同。仓库 tag 与包版本没有一一对应的 tag（最新为 `v1.26.3133.0`/`v1.25.3131.0`），故取 `main`。
- 文本：**MIT License**，`Copyright (c) Microsoft Corporation. All rights reserved.`，要求"上述版权声明与许可声明应包含在软件的所有副本或实质性部分中"。
- 上游 `NOTICE.md`（23 176 字节，列二方组件）存在；**包内不含许可文本**，只有 `.signature.p7s`。
- 未发现针对 `OpenConsole.exe` 的额外限制条款；`NOTICE.md` 未逐行审计（未核实）。

## 2. t2 随附方案（已落盘，无二进制）

`scripts/windows/conpty/`：

| 文件 | 内容 |
|---|---|
| `fetch-conpty.ps1` | 参数化（`-OutDir`、`-Proxy`，无绝对路径、无硬编码代理）；固定版本与期望 sha256；校验包 hash → 解压两个文件 → 校验各自 sha256 → `Get-AuthenticodeSignature` 必须 Valid 且签名主体为 Microsoft；任一步失败非零退出并删除临时目录（不留半成品）。实测走通（代理模式下完整校验通过） |
| `LICENSE-ConPTY.txt` | MIT 许可原文（1116 字节，逐字） |
| `NOTICE.txt` | 来源/版本/sha256/签名结论/再分发要点/NOTICE.md 链接 |
| `README.md` | 用途、运行方式、两文件须与 app.exe 同目录、**回退到系统 conhost = 删除或改名这两个文件**、版本升级流程 |

未改 `crates/`、未新增配置项、未入库任何二进制（`vt-verify-win/conpty-dl/` 已在 `.gitignore`）。

## 3. t3 三条件对照（release）

条件：**S** = 系统 conhost（无 sideload）；**W** = WezTerm 20240203 sideload；**N** = 官方 1.25.261109001（`fetch-conpty.ps1` 下载并校验）。

### 3.1 a) e2e 矩阵

| 套件 | S | W | N |
|---|---|---|---|
| `run_all`（14 例） | **14/14** | 13/14（`d-backspace-arrows`） | 13/14（`d-backspace-arrows`） |
| `run_2b_clip`（11 例） | 9/11（一次 m2/m3 抖动）；重跑 11/11 | **11/11** | **11/11** |
| `run_2b_mouse`（6 例） | **6/6** | **6/6** | **6/6** |
| `run_2c_win`（8 例） | **8/8** | **8/8** | 8/8（首轮）/ 7/8（复跑 `c2a` 抖动） |

- `d-backspace-arrows` 在 W/N 各出现一次，dump 显示 `C:\Users\Admin>cd`（首个 `a` 未到达）——与 3C/3H 记录的注入抖动同因；S 本轮未出现。
- `w1-wheel-autofollow` 原断言已过时（4K 起"键入输入会回底"），本轮改为用 `ping` 后台输出做暂停判据后 S/W/N 全过；`w3` 首轮因首个按键丢失失败（dump 显示 `,1,300) do @echo LN%i`），重跑通过。

### 3.2 b) 折行是否物化（200 字符单行，原始字节）

| 条件 | 分段长度 | 是否出现 CRLF |
|---|---|---|
| S | `80 / 80 / 43` | **是（物化）** |
| W | `200` | 否 |
| N | `200` | **否** |

### 3.3 c) 序列透传（原始读端探针，bash 与 pwsh 两种发生器结论一致）

| 序列 | S | W | N |
|---|---|---|---|
| `ESC[7m` + 空格 | 改写为 `ESC[30m`+`ESC[47m` | **原样到达** | **原样到达** |
| `OSC 4` | 到达 | 到达 | 到达 |
| `OSC 104` | **丢弃** | **丢弃** | **到达（N 独有）** |
| `OSC 0` | 到达 | 到达 | 到达 |

### 3.4 d) pwsh 7 连续启动 20 次存活率

| 条件 | 存活 |
|---|---|
| S / W / N | **20/20 / 20/20 / 20/20** |

### 3.5 e) 粘贴精确送达（真实路径，各 3 次）

| 条件 | 1 MB | 5 MB | 5 MB 粘贴中 Ctrl-C 送达 |
|---|---|---|---|
| S | 1048576 ×3 | 5242880 ×3 | 65 531 字节 |
| W | 1048576 ×3 | 5242880 ×3 | 65 531 字节 |
| N | 1048576 ×3 | 5242880 ×3 | 65 531 字节 |

### 3.6 f) 2 M 行吞吐（120×30，各 1 次）

| 条件 | 总时长 | render/present 中位 | feed | chan |
|---|---|---|---|---|
| S | 330 s（3E/3J 同口径历史值） | 4 / 1 ms | 442–470 KB/s | 1 |
| W | 229 s（3D）/ 467 s（3E） | 3–4 / 1 ms | 371–387 KB/s | 1 |
| N | **355 s** | 3 / 1 ms | 450 KB/s | 1 |

### 3.7 g) 启动（各 5 次，ms）

| 条件 | first_frame 中位 / 最小 | first_pty_byte 中位 / 最小 |
|---|---|---|
| S | **1673 / 1487** | 1683 / 1496 |
| W | 1878 / 1566 | 1842 / 1576 |
| N | 1787 / 1716 | 1759 / 1692 |

### 3.8 h) 窗口关闭（各 3 次）

| 条件 | 无粘贴 | 粘贴 5 MB 中 | stderr |
|---|---|---|---|
| S | 431 / 437 / 443 ms | 2481 / 2491 / 2488 ms | 3/3 `pty writer: join 超时（2s），已分离线程（对端长期不读）` |
| W | 444 / 507 / 829 ms | 2708 / 2773 / 2830 ms | 同上 3/3 |
| N | 420 / 426 / 1350 ms | 2465 / 2503 / 2529 ms | 同上 3/3 |

### 3.9 i) take_writer 路径（ping -t 前台 + 5 MB 粘贴，loop_gap 最大值）

| 条件 | 样本 | 结果 |
|---|---|---|
| S | 3 次 | 223（首次，噪声）/ **75 / 76** |
| W | 1 次 | 75 |
| N | 1 次 | **86**（<100） |

### 3.10 启用判定（工程判断）

门槛：N 在 a、d、e、h、i 上无新增失败，且 b 或 c 至少一项优于 S。

- a：N 13/14 + 11/11 + 6/6 + 8/8，唯一失败 `d-backspace-arrows` 为已知注入抖动（S 本轮 14/14，但该用例在 3C/3H 亦失败过，非 N 引入）。
- d 20/20、e 三档全等、h 全部退出且日志一致、i 86 ms < 100 ms。
- b：N 不物化折行（S 物化）→ **优于 S**；c：N 保留 `ESC[7m` 且 **`OSC 104` 可达**（S/W 均丢弃）→ **优于 S**。
- 结论：**满足启用门槛，建议随附官方 1.25.261109001**。代价（同时记录）：启动 first_frame 中位比 S 高约 110 ms、2 M 行吞吐比 S 慢约 7%。

## 4. t4 app 空闲内存基线（release，60 s 空闲，3 次/配置）

| 配置 | 10 s（WS/Private/Paged MB） | 30 s | 60 s |
|---|---|---|---|
| A 默认（Consolas + Microsoft YaHei 回退） | 878–880 / 896–902 / 896–902 | 同 10 s | **878–880 / 896–902** |
| B 仅 Consolas（无回退） | 189–190 / 180 / 180 | 同 | **189–190 / 180** |
| C 仅 Courier New（无回退） | 207 / 198–199 / 198–199 | 同 | **207 / 198–199** |

- 三次采样完全持平（10 s 就已稳定），说明 3J 里 1518 MB 的 RSS 是**空闲基线本身**（默认配置）叠加粘贴余量，不是粘贴导致。
- 数据直接支持：**去掉 YaHei 回退后空闲 WS 从约 879 MB 降到约 189 MB（差约 690 MB）**；Courier New 无回退为 207 MB。
- 对照：Linux CPU 后端基线约 215 MB（来源：PHASE4K-LINUX）；本机无回退时 189–207 MB 与之同量级。

## 5. t5 字体文件打开成本（10 次，中位/最小，ms）

| 文件 | 路径 | 首次读到首字节 | 完整读取 | 热打开 |
|---|---|---|---|---|
| `consola.ttf` | `C:\Windows\Fonts` | 0.16 / 0.13 | 0.95 / 0.60 | 0.29 / 0.20 |
| `consola.ttf` | 临时目录副本 | 0.15 / 0.10 | 1.59 / 0.59 | 0.25 / 0.18 |
| `msyh.ttc` | `C:\Windows\Fonts` | 0.15 / 0.10 | 9.78 / 7.32 | 0.34 / 0.26 |
| `msyh.ttc` | 临时目录副本 | 0.17 / 0.11 | 10.83 / 9.29 | 0.36 / 0.27 |

- 系统路径与临时副本差别在噪声内 → **文件系统路径/安全软件扫描不是那 ~137 ms 的来源**，该假设（3J 待验证）**本轮数据不支持**。
- 注意 `msyh.ttc` 完整读取约 10 ms，与 3J 的 `parse_ms ~24 ms`、组内未归属 ~137 ms 都不在同一量级。

## 6. 未解决问题

1. N 条件下 `d-backspace-arrows` 与 `c2a` 各出现一次失败（注入抖动），未定位到与 ConPTY 版本相关的证据。
2. N 的启动代价（+110 ms 中位）来源未拆分（conpty.dll 加载 vs OpenConsole 宿主启动）。
3. 字体组内 ~137 ms 未归属耗时仍无解释（t5 已排除"首次打开被扫描"这一路径）。
4. 过程事故：本轮曾按 PID 结束两个 `OpenConsole.exe`，可能属于用户 WezTerm 会话（§0）。

## 7. 对共享代码的请求

1. **（观测增强）** 3J 提出的字体组内 begin→seg、seg→cache 两段仍无打点；本轮 t5 已排除"文件首次打开被扫描"，建议在渲染器内部补点定位。
2. **（信息）** 3E/3K 结论：官方 1.25 ConPTY 下 `OSC 104` 会到达 app，程序侧若依赖"改色后复位"的行为，在系统 conhost 与 WezTerm 下仍不可用（平台差异，已记录）。
3. 无共用代码改动需求。

## 8. 交付物与证据

- 随附方案：`scripts/windows/conpty/{fetch-conpty.ps1, LICENSE-ConPTY.txt, NOTICE.txt, README.md}`（无二进制）。
- 脚本：`vt-verify-win/{run_3k_cond,run_3k_i,run_3k_mem,run_3k_fontopen}.ps1`；e2e 驱动新增 `official` 条件（`scripts/windows/e2e/run_conpty_condition.ps1`）。
- 日志：`vt-verify-win/logs/{3k-runall-*,3k-clip-*,3k-2c-*,3k-mouse-*,3k-wrap-*,3k-seq-*,3k-start/*,3k-cond-official}.log`；`vt-verify-win/logs/conpty-seq/{S,W,N}-*`。
- 下载物与暂存：`vt-verify-win/conpty-dl/`（gitignore）。

## 9. 推送与打 tag（ls-remote 实际输出）

| 引用 | hash |
|---|---|
| `refs/heads/win` | `b42f02580cdcce7b99e34b69ff02c15317cdb992` |
| `refs/tags/phase3k-win-done`（tag 对象） | `8dac33272255579d3a9f759104b596d83e5bff7a` |
| `refs/tags/phase3k-win-done^{}`（解引用 commit） | `b42f02580cdcce7b99e34b69ff02c15317cdb992` |

- `git push origin win`：`5ffd6fa..b42f025  win -> win`；tag 为新建后 push（`* [new tag]`）。未使用 force，未移动任何已有 tag。

## 10. 手动清单（≤3）

1. 用 `fetch-conpty.ps1` 在目标机器取一次官方包并放到 app.exe 旁，确认启动后 `OpenConsole.exe` 由官方版本提供（文件版本 1.25.2611.09001）。
2. 若在意启动 +110 ms，可在 N 下对比 `conpty.dll` 是否存在时的首帧（本机数据见 §3.7）。
3. 用默认配置观察内存占用，若需降低基线可去掉 `font.fallback`（本机 879 MB → 189 MB，见 §4）。
