# PHASE3O-WIN：合并 4P、Windows GPU 后端构建与失败路径、CPU 无回归、kit v2

- 日期：2026-11-15（按系统时间）
- 范围：合并 `phase4p-done`（Windows 启用 wgpu GPU 后端）；在**无 GPU 的构建机**上验证 GPU 后端的构建与失败路径、CPU 路径无回归；实现授权的"两文件必须成对部署"警告；产出 kit v2（含 GPU 构建）并在本机 `-Quick` 与默认参数各试跑一次。**除 t3 授权的一处 `crates/pty/src/windows.rs` 改动外，未改 `crates/`；未加依赖；二进制与 zip 均不入库。**
- 结论分级：已验证事实 / 工程判断 / 待验证假设。
- perf 日志一律 `APP_PERF_FRAMES=1` + 直接句柄重定向（`cmd /c "... 2>log"` 或 bash `2>file`）；性能与内存只用 release。

## 0. 进程处理

- 未结束任何 `OpenConsole.exe` / `conhost.exe` / `wezterm-gui.exe` / `pwsh.exe`。
- 只结束本任务自己启动、命令行核对过的 `app.exe`（kit 试用、GPU 失败路径探针、内存采样）。
- `OpenConsole` PID 集合全程恒为 `328, 9376`；`conhost` 数量从 25 变为 30，新增的 5 个经 `CreationDate` 核对均为 **08:33–11:00** 早前阶段遗留（非本轮 21:0x 的试跑），按规则只记录、不处理。

## 1. t1 合并与构建

| 项目 | 结果 |
|---|---|
| `phase4p-done`（`git ls-remote`） | `d2fc1da9abdc8590622781dafa5f2c199975dd68`，只有一行（**轻量 tag，无 `^{}`**） |
| 合并 | `git merge --no-ff d2fc1da`，**0 冲突**，合并 commit `90a75a9` |
| Windows 专属文件整体 sha256 | `e8f463d22df9ece96078607c09fd20e88a1f9c5805adf542917ec90a48a1dfe3`，**合并前后一致** |
| release 构建（含 wgpu dx12/vulkan） | **通过，222 s（3m40s），零告警** |
| app.exe 体积 | **18 084 829 B**（3N 为 7 866 405 B，**+10 218 424 B ≈ +10.2 MB**，即 wgpu/DX12/Vulkan 后端） |
| clippy `-D warnings`（dev / release） | **0 / 0**（3N 的 `unused import: eperf` 已被 4P 的 GPU 路径实际使用而消失） |
| `cargo test --workspace` | **196 passed / 0 failed / 2 ignored**，rc=0（耗时 536 s） |
| `cargo tree --target x86_64-pc-windows-gnu` | **`wgpu v30.0.2`**；启用特性 = **`dx12`、`vulkan`、`wgsl`**（`default-features = false`） |
| GPU 相关测试在本机的状态 | **全部 SKIP 并打印原因**：`SKIP: 无可用真实 GPU 适配器（未找到真实 GPU 适配器（仅软件实现可用））`；测试结果本身仍是 `ok`。覆盖 `gpu_parity`（8 例）、`inverse_pixels`、`baseline_alignment`、`glyph_clip`、`cursor_viewport_pixels` 的 `*_gpu` 用例 |
| `frame_bytes_identical` | 仍 **SKIP**（字体指纹 340712 vs 343140，见 3N 请求 2），`frame_hash=507485fb7945fb8f` |

## 2. t2 GPU 失败路径（无 GPU 机器）

全部用最小命令（`cmd /c "<wrapper>.cmd"`，wrapper 内 `cd /d`、`set APP_PERF_FRAMES=1`、`2>log`）：

| 用例 | 退出码 | 启动→退出耗时 | 错误原文（stderr 关键行） | panic/挂起 |
|---|---|---|---|---|
| `backend="gpu"`（默认环境） | **1** | **1.5 s** | `create_renderer(gpu): render error: backend error: 未找到真实 GPU 适配器（仅软件实现可用）` ／ `Error: Render(Backend("未找到真实 GPU 适配器（仅软件实现可用）"))` | 无 |
| `backend="gpu"` + `WGPU_BACKEND=dx12` | **1** | **0.8 s** | 同上（逐字相同） | 无 |
| `backend="gpu"` + `WGPU_BACKEND=vulkan` | **1** | **0.7 s** | 同上（逐字相同） | 无 |
| `backend="auto"` | 不退出（窗口常驻） | 8 s 后仍在运行 | 无错误；`[perf] backend=auto → cpu（Windows 上 auto 不尝试 GPU）`、`renderer_build_ms=… backend=cpu`；日志中 **无 `gpu_init_ms`** | 无 |

- 适配器枚举（`APP_PERF_FRAMES=1` 下 `backend="gpu"` 的原文，仅一行）：`[perf] gpu_adapters name='Microsoft Basic Render Driver' backend=Dx12 type=Cpu driver='10.0.19041.4355' driver_info='' vendor=0x1414 device=0x008c` —— 只有 **type=Cpu** 的软件适配器，被过滤后报"未找到真实 GPU 适配器"，与行为一致。
- `auto` 时 app 正常出窗、ws 269 MB、关窗退出正常；3 次 `backend="gpu"` 均在 2 s 内干净失败，**没有 panic、没有挂起**。

## 3. t3 CPU 路径无回归

| 项目 | 3O（4P 合并后） | 对照 | 判定 |
|---|---|---|---|
| `first_frame_ms`（10 次，直接句柄） | 中位 **414 ms**、最小 **320 ms**（样本 415/415/415/402/408/417/320/420/412/411） | 3M 419 / 395 | 无回归（且在噪声内更快） |
| `first_pty_byte_ms` | 中位 698 / 最小 661 ms | 3M 726 / 662 | 无回归 |
| 默认配置空闲内存（60 s 平台值，3 次） | ws **268 / 270 / 270 MB**，priv 268–270 MB | 3M 252 MB | 高约 16 MB（记录，未归因） |
| 启动期峰值（20 ms 采样，3 次） | **406–416 MB @ 0.39–0.54 s**，随后回落平台 | 3M 峰值 = 平台（无过冲） | **新增约 +150 MB 瞬时过冲**，记录未归因 |
| `run_all`（14 例） | **14/14** | — | 无回归 |
| `run_2b_clip`（11 例） | **11/11** | — | 无回归 |
| `run_2b_mouse`（6 例） | **6/6** | — | 无回归 |
| `run_2c_win`（8 例） | **8/8** | — | 无回归 |
| 粘贴 1 MB ×3 / 5 MB ×3 | **1048576 / 5242880 全部精确** | 3K/3M 同值 | 无回归 |
| 5 MB 粘贴中 Ctrl-C 送达 | **65 531 B** | 3K 65 531 | 无回归 |
| `loop_gap` 最大值（ping -t + 5 MB 粘贴） | **97 ms**（< 100） | 3K 86 ms / 3M 75–86 ms | 无回归 |
| 随附 conpty 折行字节 | N 条件 **单段 200 字符、无 CRLF** | 3K/3M 同结论 | 无回归 |
| pwsh 连续启动 20 次 | **20/20 存活** | 3K 20/20 | 无回归 |

### 3.1 授权的共用代码改动（单独提交 `fd52b80`，`win:` 前缀）

- 文件：`crates/pty/src/windows.rs`（Windows 专属）+ `crates/pty/tests/pty_windows.rs`。
- 内容：新增 `pub fn conpty_pair_incomplete(dir: &Path) -> bool`（纯判定）与 spawn 前的部署自检；命中"目录里有 `conpty.dll` 但没有 `OpenConsole.exe`"时向 stderr 打印**默认可见**（不经过任何 perf 开关）的一行中文警告，**不改变启动路径与其他行为**。
- 警告原文（实测）：`[pty] 警告：程序目录存在 conpty.dll 但缺少 OpenConsole.exe——两文件必须成对部署；本次将改用系统 conhost（折行会被物化、ESC[7m 会被改写）。`
- 证据：
  - 单元测试 `conpty_pair_detection_needs_both_files`（临时目录构造"两文件齐/只有 dll/只有 exe/都没有"，断言只有"只有 dll"命中）**通过**；
  - 真实部署场景：`vt-verify-win/3o-warn/` 放 app.exe + conpty.dll（无 OpenConsole.exe），启动后 stderr **出现该警告行**；
  - 完整成对目录（`vt-verify-win/3o-warn-full/`）启动，stderr **无该警告**（0 行）。
- 改动后 release 重新构建：零告警；`cargo test --workspace` 仍全绿。

## 4. t4 kit v2

### 4.1 相对 v1 的改动

1. **P2 真正执行**：不再依赖"本构建无 GPU 后端"这一前提，`backend="gpu"` 变体（默认 / `WGPU_BACKEND=dx12` / `=vulkan`）在本机构建上会真实启动并失败，P2 对每个变体给出 **N/A + 原因**：
   - `default: auto uses the CPU path on Windows by design (no GPU attempt)`
   - `dx12: no usable GPU adapter (only software adapters were enumerated)`
   - `vulkan: no usable GPU adapter (only software adapters were enumerated)`
2. N/A 判定顺序改为"**等进程退出或 12 s 超时 + 日志不再增长**后再读日志"，修掉了上一版"原因文字读早了"的竞态（首次 v2 试跑里 default/dx12 显示成 `exited on start`）。
3. 命中 GPU 时额外记录 `gpu_adapters`、`gpu_skip_software`、`gpu_adapter_ms`、`gpu_atlas_ms`、`gpu_begin_ms`（键名已对齐产品实际打印的名称）。
4. summary 标题改为 kit v2。

### 4.2 包

| 项 | 值 |
|---|---|
| 目录 | `vt-verify-win/tonight-kit-v2/`（gitignore），12 文件，**19 390 489 B** |
| zip | `vt-verify-win/tonight-kit-v2.zip`（gitignore），**6 618 947 B** |
| zip sha256 | **`98e4c5391ca21c84e7817fbdcf9f3c4efcc77c9b010cf4a14fe8501fa3a8ea2a`** |
| app.exe | 18 087 652 B，sha256 `addb0723…`（含 GPU 后端） |
| run-tonight.ps1 / lib.ps1 | 35 512 B（`f7e9a68d…`）/ 17 990 B |
| 其余 | conpty.dll `e971856f…`、OpenConsole.exe `b17e48b0…`、LICENSE-ConPTY.txt、NOTICE.md `e7fbaade…`、三个 cfg、README-TONIGHT.md、MANIFEST.txt |

### 4.3 试跑（解压到 `…\kt v2 中文 路径\`，从 `C:\Windows` 启动）

| 运行 | 参数 | 总耗时 | P0 | P1 | P2 | P3 | P4 | zip |
|---|---|---|---|---|---|---|---|---|
| 试跑 | `-Quick` | **5.4 min** | PASS 26.1s | PASS 211.7s | **N/A** 16.0s | PASS 67.1s | PASS 1.8s | 78 408 B |
| 正式 | 默认（浸泡 5 min） | **10.1 min** | PASS 32.4s | PASS 224.9s | **N/A** 16.0s | PASS 332.1s | PASS 1.9s | 82 135 B |

默认运行的关键数字（release）：

- `first_frame_ms [cpu]` n=5 中位 **425 ms** / 最小 311 ms；
- 空闲 ws **271.5 MB** / priv 274.9 MB / 句柄 259 / 线程 10；
- 吞吐 120×30 **53.4 s**（feed 424 553 B/s、render 4 ms、present 1 ms）；200×60 **73.5 s**（feed 360 471 B/s、render 12 ms、present 6 ms）；
- 最小化→恢复、遮挡→移走对比 **diff 0**；20 次缩放对比 diff_ratio 3.3%（12 列结构性差异 = 滚动条宽度，同 3N，只记录）；
- 浸泡 10 次采样 ws 268.6 → **272.6 MB**，无 panic，关窗正常退出；
- 全程 `OpenConsole` PID 集合保持 `328,9376`，`app_left=0`，未结束任何非自己启动的进程。

### 4.4 本轮修掉的 kit 缺陷（2 项）

1. P2 的 N/A 原因判定在进程尚未退出时读日志 → `default`/`dx12` 被误记为 `exited on start`；改为等退出/超时且日志不再增长后再读。
2. summary 标题仍写 "kit v1"。

## 5. 未解决问题

1. 本机无真实 GPU 适配器，**P2 的成功路径（GPU 渲染、GPU 截图对比、GPU 浸泡）仍未真实执行**；今晚目标机是首次真实运行，P2 代码路径属"待验证假设"。
2. 默认配置空闲内存 268–270 MB（3M 252 MB）+ 启动期 406–416 MB 的瞬时过冲（3M 无过冲）——**只记录，未归因**（可能是 GPU 后端链接后的静态/驱动初始化，未验证）。
3. 20 次缩放后 3.3% 像素差（12 列 == 滚动条宽度）仍未归因。
4. `frame_bytes_identical` 在本机仍 SKIP（3N 请求 2 未处理）。
5. `conhost` 数量 +5 的遗留进程为早前阶段产生，非本轮，未处理（按规则只记录）。

## 6. 对共享代码的请求

1. **（功能，P2）** 只剩 `conpty.dll` 时的静默回退已有默认可见警告（本轮 `fd52b80`）；建议 README/部署文档同步一句"两文件必须成对部署"。
2. **（测试，P2，沿用 3N 请求 2）** `frame_bytes_identical` 的 `GOLDEN_FINGERPRINT` 绑定单一 DejaVu 尺寸，Windows 侧恒 SKIP；建议改指纹集合或分平台期望值。
3. **（观测，P3）** 启动期内存峰值（406–416 MB）在 `APP_PERF_FRAMES=1` 下没有任何分段打点可对齐，若要在目标机定位，需要在 renderer 初始化前后补内存采样点。

## 7. 手动清单（≤3）

1. 目标机（真实 GPU）解压 `tonight-kit-v2.zip` 后运行默认参数，重点看 P2：应出现适配器名/后端（dx12 或 vulkan）、`gpu_init_ms`，且 P2 为 PASS 而非 N/A；GPU 截图为 `S_gpu_dx12.png` / `S_gpu_vulkan.png`。
2. 把 `app.exe` 与两个 conpty 文件放在同一目录；若要验证新警告，删掉 `OpenConsole.exe` 再启动，stderr 应出现 `[pty] 警告：…两文件必须成对部署…`。
3. 回传 `results\tonight-*.zip`，并附 README 的三条手动项观察（切显示器/改缩放、GPU 观感与功耗、中文输入法与 Ctrl+Shift+C）。

## 8. 推送与 tag（ls-remote 实际输出）

见文末补记。
