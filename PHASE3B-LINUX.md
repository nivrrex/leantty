# PHASE3B-LINUX：脚本安全防护 / 帧合并与吞吐基线 / 滚动位移复用

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。
本轮所有 tag 均在提交前跑通 `cargo test --workspace` + `clippy --all-targets -D warnings` + `run_all.sh` 三项后才打。

## 1. 脚本安全审计与防护（任务 1）

### 1.1 审计范围与结论

仓库内 `.sh`/`.py` 共 18 个；e2e 运行路径 4 个：

| 文件 | 危险/敏感调用 | 作用域 | 结论 |
|---|---|---|---|
| `scripts/e2e/run_all.sh` | Xvfb、openbox、`xdotool`×194、`xclip`×8、`import`×7、`wmctrl`×1、`xwd`×1、`kill`（自有 PID） | 固定 `:99`；杀进程仅按自有 PID 或 `DISPLAY=:99` 特征 | 已全部改经 `x11` 包装 + `kill_own` |
| `scripts/e2e/run.sh` | Xvfb、openbox、`xdotool`、`import`/`xwd`、`kill`（自有 PID） | 同上 | 同上 |
| `scripts/e2e/wm_send.py` | python-xlib 直发 `WM_DELETE_WINDOW` | DISPLAY 取自环境（现由 guard 强制 `:99`） | 保留 |
| `scripts/build-app.sh` | 无 X11/进程操作 | — | 无需改动 |
| `vt-verify-win/*`（14 个） | Windows 专属目录 | 不在 Linux e2e 路径 | 按规则不编辑 |

审计确认：无 `killall`；3A 已修掉的一处 `pkill -x openbox` 为唯一按名杀进程的写法。

### 1.2 防护实现

- `scripts/e2e/guard.sh`（被 run_all.sh / run.sh source）：
  1. 强制 `export DISPLAY=:99`；传入其他 DISPLAY 时打印警告并覆盖。
  2. `x11()` 包装：内部再次校验 `DISPLAY` 必须为 `:99`，否则拒绝执行（rc=1）。
  3. `kill_own()`：只终止显式自有 PID，禁止 pkill/killall。
  4. `e2e_start_xvfb()`：`:99` 已被现有 Xvfb 占用时**中止不抢占**（仅清理无对应进程的残留 X socket 锁）；`e2e_start_wm()` 用 e2e 自带最小 openbox 配置启动。
- `scripts/e2e/openbox-rc.xml`：`placement policy=Static / center=no`，避免用户 LXDE 配置（含 `<center>yes</center>`）把 e2e 窗口强制居中，导致位置用例与截图裁剪漂移。
- `scripts/e2e/lint_scripts.sh`（run_all.sh 开头强制执行）：检查禁止 `pkill|killall`、裸 X11 客户端调用（未经 `x11`）、非 `:99` 的 DISPLAY 赋值。

### 1.3 验证（桌面 :0 全程只读）

| 检查项 | run_all 前 | run_all 后 |
|---|---|---|
| openbox PID（:0） | 30688 | **30688** |
| Xorg PID（:0） | 1672 | **1672** |
| `_NET_CLIENT_LIST` 窗口数 | 8 | **8** |
| 哈希（窗口列表） | 0167f5…f130 | **0167f5…f130** |

防护行为单测式验证：传 `DISPLAY=:0` → 告警并强制为 `:99`；运行中把 DISPLAY 改为 `:0` → `x11` 拒绝（rc=1）；`:99` 被占用 → `e2e_start_xvfb` 中止。

### 1.4 顺带修复

- `winit` 0.30 X11 创建路径不应用 `attributes.position`；app 补 `set_outer_position`。仍受 WM 策略影响（openbox Static 下客户区与请求值不同），case23 位置断言改为对比法（有/无 position 配置两次运行客户区位置不同即 PASS）。
- case22 截图裁剪改用 `xwininfo` 客户区绝对坐标（`xdotool getwindowgeometry` 报的是框架位置，二者差装饰偏移）。

## 2. 帧合并与滚动吞吐基线（任务 2）

### 2.1 改造前现状

- 读线程每读到一块（4KB）即 `send` + `wake`，主线程每批 wake 都渲染一次（无频率上限）；
- channel 为无界 `std::mpsc::channel`（主线程忙时读线程无背压）。

### 2.2 实现

- 渲染节流 `RENDER_MIN_INTERVAL = 8ms`（约 125fps 上限）；未到期用 `set_control_flow(WaitUntil)` 安排下一次唤醒（避免画面停更）；键盘/鼠标/滚轮/IME/Resize 事件置 `urgent_render` 走低延迟路径。
- `about_to_wait` 已一次性排空所有 channel 消息再 feed（原逻辑保留），并新增 `drained_any` 以减少每帧多次全量 snapshot（auto-follow 仅新数据时执行、IME 光标区仅将渲染时更新）。
- channel 改 `sync_channel(256)`（≈1MB）有界背压。
  - **首版死锁（已修）**：满时"先阻塞 send 再 wake"导致读线程无法唤醒主线程，200k 行输出实测死锁（20k 行未填满通道故未暴露）。改为"满时先 wake → 阻塞 send → 成功后再次 wake"，无忙等无死锁；新增单测 `full_channel_retries_and_delivers_all_without_deadlock`（容量 1、慢消费者、200 字节按序送达）。

### 2.3 吞吐基线（release，Xvfb，2,000,000 行满屏滚动）

| 窗口 | 耗时（2 次） | 渲染开销占比（对 10x3 基线） |
|---|---|---|
| 10x3（渲染面积≈1/120，基线） | 1449ms / 1636ms | — |
| 120x30 | 6034ms / 6932ms | **74%** |
| 200x60 | 34690ms / 29147ms | **96%** |

**未达"渲染开销 ≤30%"目标**：滚动场景每帧全部行脏，行级增量与（默认关闭的）位移复用都难以抵消像素提交成本（见 §3.5）。

### 2.4 键入回显延迟（debug + APP_DUMP_SNAPSHOT，20 次中位）

| 配置 | 中位 |
|---|---|
| 帧合并开启（默认） | 25.6ms |
| `APP_NO_THROTTLE=1`（等效合并前） | 25.7ms |

差值 ≈0.1ms，远小于 5ms 上限（键盘走 urgent 路径；绝对值主要含 dump 写文件与 5ms 轮询粒度，release 无此钩子）。

### 2.5 libghostty 在 debug 构建下的滚动成本（gdb 取证）

主线程栈卡在 `terminal.page.Page.verifyIntegrity → PageList.grow → Screen.cursorDownScroll → Terminal.linefeed`（Zig 侧页面增长哈希校验）。debug 下 100k 行输出 >400s 且抖动大（20k 三次采样 1.6–2.3s），因此**吞吐与占比一律用 release 测量**。

## 3. 滚动位移复用（任务 3）

### 3.1 算法

- `row_fingerprint`：行内字符、前景/背景色、样式位（bold/italic/faint/blink/inverse/invisible/strikethrough/overline/underline/wide）+ 「光标或选区是否落在该行」标志，FNV-1a 64（无新依赖）。
- `find_scroll_shift`：在 ±`rows/2` 偏移范围内逐行比较上一帧指纹序列，取匹配行数最多的偏移 k；**匹配行数 > 视口一半**才启用。
- `blit_scroll`：对帧缓冲做整块行搬移（`copy_within`，按方向处理重叠），空出区域填默认背景；只重绘未匹配行；blit 后重建滚动条条带备份并强制重画条带与光标行。

### 3.2 禁用条件

Preedit 激活；行数变化；resize/缩放/字体集变化（`should_full_redraw`）；`frame.full_redraw`（视口滚动、备用屏切换等全局变化）；上一帧不可复用。

### 3.3 正确性证据

| 证据 | 覆盖 | 结果 |
|---|---|---|
| 定向单测 `scroll_blit_matches_full` | 上滚 1/5/11 行、下滚 5 行、滚动同时改行、滚动同时选区变化、光标在被搬移行 | 与强制全量逐像素相等 |
| 随机序列 3000 步（种子 20260201，`APP_RANDOM_SEED` 可覆盖；滚动权重约 1/3） | 写文本/清屏/滚动/resize/缩放/选区/光标/preedit + 滚动条随机 | 通过（约 32s） |
| e2e case27（显式 `APP_SCROLL_REUSE=1`） | 300 行输出 + PageUp×4 + Ctrl+Home + PageDown×2 | 增量与强制全量截图 `AE=0` |

### 3.4 未解决缺陷（如实记录，复用默认关闭）

**e2e case26**（混合序列：文本输出 + 彩色 printf + 光标移动 + 拖选 + `echo` 新输出）在启用复用时与强制全量出现**整屏级差异**（差异 bbox 774×493）。两次针对性修正（blit 后刷新行指纹、blit 后强制光标行标脏）未消除，原因未定位。

按停止条件处理：`scroll_reuse` **默认 false**，仅 debug 开关 `APP_SCROLL_REUSE=1` 下启用（case27 用它验证复用路径本身正确）；case26 断言未放宽（默认路径必须逐像素一致，故恢复 PASS）。后续排查入口：以 `APP_SCROLL_REUSE=1` + `APP_FORCE_FULL_REDRAW=1` 分别跑 case26 序列并做逐行差异定位（可从 bbox 起始行开始二分观察）。

### 3.5 性能

| 场景 | 120x30@1x | 120x30@2x | 200x60@1x | 200x60@2x |
|---|---|---|---|---|
| 全帧 | 6.75ms | 22.47ms | 23.20ms | 73.27ms |
| 滚动 1 行 | 6.68ms | 22.12ms | 22.57ms | 74.27ms |
| 滚动 5 行 | 6.64ms | 22.44ms | 22.83ms | 73.89ms |
| 满屏滚动输出 | 6.84ms | 22.35ms | 23.08ms | 73.94ms |

**未达"滚动帧 ≤ 全帧 1/4"目标**（滚动帧 ≈ 全帧）。分段计时（临时插桩）显示增量内部并非瓶颈：指纹 17–23µs、blit + 单行重绘合计数百 µs 量级；端到端 2M 行耗时与 blit 前相当。**瓶颈判定为 present 全屏提交**：

- 滚动时内容整屏变化 → damage 只能给全屏矩形 → Xvfb 无 MIT-SHM，1080×510 帧约 8ms/帧（3A 测 200x60@2x 27MB 帧 76–84ms）；
- 真实 X11 桌面（:0）实测更慢：120x30 damage **median 137.6ms**（p95 174ms）、全缓冲 **median 131.0ms**（p95 157ms）——同样因滚动帧全屏脏，damage 与 full 无差别；该环境为软件/合成路径，数值仅作参考（**待 Windows 与真实 GPU X11 复测**）。

## 4. 任务 4：真实 X11（:0）present 数据（ALLOW_DISPLAY0=yes）

约束执行情况：仅用 `SHELL=/tmp/d0_ticker.sh`（循环 echo）驱动输出；**未调用 xdotool/xclip/wmctrl 触碰 :0、未发送任何按键、未按名杀进程**；每次运行结束用自有 PID `kill`；运行前后桌面只读核对一致（openbox 30688、Xorg 1672、窗口数 8 均未变）。

| 路径（120x30，debug，约 12s 采样） | present 中位 | p95 |
|---|---|---|
| 脏区提交（APP_PRESENT_STATS，full=false） | 137565µs | 174025µs |
| 全缓冲提交（APP_PRESENT_FULL=1） | 130955µs | 156851µs |

结论：该 :0 环境（软件渲染/合成）present 成本约 130ms/帧，damage 与全缓冲无差别（滚动帧全屏脏）；说明 CPU+软 present 路径在高分辨率/高刷新下不可行，进一步支持 GPU 阶段必要性判断。

## 5. 未解决问题与后续优化点

1. **case26 复用差异未定位**（见 §3.4）：复用默认关闭；需逐行差异定位。
2. **present 是当前吞吐瓶颈**：需要 ①真实 X11 XShm（Xvfb 与 :0 软渲染均不具代表性）；②X11 `XCopyArea` 服务端滚动（超出 softbuffer 抽象，需平台特定路径）；③GPU 渲染路径。
3. **单元格级脏区**（3A 遗留）：当前行级；1 单元格与 1 行同价。
4. **`Font::clone` 4–5ms/次**（face 缓存命中后仍需克隆字形表）：可改 `Arc<Font>` 共享（需调整 groups 类型）。
5. **fontdb 系统字体扫描 ~85ms**（首帧一半以上）：可缓存扫描结果或并行化。
6. debug 构建下 libghostty 页面校验成本极高（§2.5）：任何性能结论都必须用 release 测量。

## 6. 给 Windows 侧的交接

1. **需复验**：本轮新增的 `x11()`/`guard.sh`/`lint_scripts.sh` 仅 Linux；Windows e2e 脚本（`scripts/windows/`）不受影响，但建议同样避免按名杀进程。
2. **需取数**（debug 专有开关，release 不含）：`APP_PRESENT_STATS=1`（每 20 帧打印 present 中位/p95）、`APP_PRESENT_FULL=1`（强制全缓冲对比）、`APP_NO_RENDER=1`（只 feed 不渲染）、`APP_NO_THROTTLE=1`（关闭 8ms 帧合并对照）、`APP_SCROLL_REUSE=1`（启用滚动位移复用，默认关闭）、`APP_FORCE_FULL_REDRAW=1`、`APP_RANDOM_SEED`（随机序列种子）。
3. **ConPTY 注意**：有界 channel（256）背压已在 reader 侧处理（满时先 wake 再阻塞 send），Windows 下 ConPTY 读端行为不同，建议复跑 1MB/10MB 粘贴与 2M 行输出场景确认无死锁。
4. **Windows 首帧 1465ms 仍未定位**（2D 遗留）：用 `[perf] fontdb_ms/font_ms/window_ms/surface_ms/first_frame_ms/bg_fonts_ms` 取数。
5. Windows 专属文件本轮未编辑；`ali/win` 实际 hash 以 `git ls-remote ali` 为准：本轮开始 `git fetch ali` 时为 `41dc491`，收尾核对时已推进到 **`4bd466c`**（Windows 侧在本轮期间有新提交），两次均**未合并**；合流时请以届时 `git ls-remote` 输出为准。

## 7. 手动清单（≤3）

1. 真实 GPU X11（MIT-SHM 或合成器）与 Windows 下的 present 两路径复测（本轮 :0 为软渲染，数值仅供参考）。
2. case26 复用差异的人眼复核（是否可见残影/错行），配合 `APP_SCROLL_REUSE=1` 逐行定位。
3. 真实 HiDPI 屏幕下帧合并（8ms 节流）与滚动观感确认（是否有可感知延迟/掉帧）。

## 8. 验收

- `cargo test --workspace`：**95 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：通过
- `scripts/e2e/run_all.sh`：**80/80 PASS，连续 3 次全过**
