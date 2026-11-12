# PHASE2D-LINUX：合并 Windows 成果 / 粘贴排空修复 / 主字体评估 / CPU 帧耗时

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。

## 1. 合并 ali/win（任务 1）

- **ali/win 实际 hash = `3811f91`**（Windows 报告里的 `a4e6233` 是其父提交 `2B-win: clipboard/selection/mouse/wheel/IME/startup e2e + pwsh ConPTY matrix`）。
- merge-base = `d278e76`（phase2b-done）。
- **shared 文件改动审查**：win 侧相对 merge-base 共 298 个文件；排除 Windows 专属路径（`vt-verify-win/`、`scripts/windows/`、`docs/WIN-NOTES.md`、`PHASE2A-WIN*.md`、`PHASE2B-WIN.md`）后，shared 代码文件**只有 `crates/app/src/clipboard.rs`**（提交 63c8c7f：arboard X11 扩展 API 拆 `cfg(unix)` / `cfg(not(unix))`，Windows 下 PRIMARY get/set 返 Err）。无清单外 shared 改动 → 符合预期，执行合并。
- 合并方式：`git merge --no-ff ali/win -m "merge: win phase2a/2b (Windows ctrl-c fix, e2e, arboard cfg split)"` → 合并提交 `fc3e622`，**无冲突**。
- **合并后需修一处**（已验证事实）：win 侧 `primary_get/primary_set` 以 `&arboard::Clipboard` 接收，而 arboard 3.7 的 Linux 扩展 `get()/set()` 需要 `&mut` 接收者 → Linux 编译失败（Windows 分支因 `cfg(not(unix))` 不触碰该值而未暴露）。修复为 `&mut`（Windows 变体同步改 `&mut`，参数未使用），见提交 `b149cb1`。
- Linux 全量验证：workspace 84 测试通过、clippy `-D warnings` 0、run_all 76/76，**case12/case13（PRIMARY 写入与括号粘贴）行为不变**。tag `phase2d-t1`。

## 2. 大段粘贴排空缺陷（任务 2，P1）

**复现（Linux，已验证事实）**：`stty raw -echo; cat > /tmp/paste_out`，xclip 设 1MB 内容，右键 Paste，之后不产生任何事件；5s 时文件 **65536 字节**（1 块），产生一次鼠标移动后增至 **131072 字节**（2 块）——与 Windows 报的"约 131066 字节后停住、有事件才继续"同源。

> 复现前提：内容不含换行时，canonical 模式的行缓冲不交付字节，`cat` 观测到 0 字节；必须先 `stty raw -echo` 才能观测真实送达量。这是最初误判"粘贴未触发"的原因，已记入 DECISIONS。

**根因（已验证事实）**：`about_to_wait` 每轮只 `pop` 一块待写数据并写 PTY，写完不触发下一轮；winit `ControlFlow::Wait` 下无事件即阻塞 → 队列不再推进。

**修复（根因级，方案 a）**：pending 队列改为字节块 `VecDeque<Vec<u8>>`；每轮 `about_to_wait` 用纯函数 `drain_pending` 按预算（8 块 / 512KB）写出；队列非空时 `window.request_redraw()` 驱动下一帧。
语义：块内部分写入保留未写前缀跨轮续写；`WouldBlock` 立即返回并保留剩余（不忙等）；不可恢复错误丢弃该块并停止驱动；PTY 不可用/已关闭时丢弃剩余、不挂起事件循环。**未改 Pty trait**。

**否决方案 b（独立 PTY 写线程 + channel）**：吞吐更高，但需要把 `writer` 从 `NativePty` 移出（`Pty::writer()` 只给 `&mut dyn Write`），属 trait 变更 / 破坏 close 契约（"先释放主端再回收子进程"），按规则为停止条件，且背压与关闭顺序复杂度高，故不采用（若后续开放 trait 变更可重估）。

**验证（已验证事实）**：
- 单测 6 例（注入 writer：全写 / 预算限制 / 部分写跨轮 / WouldBlock 不 spin 且数据保留 / 致命错误丢弃 / 空队列）。
- e2e case24：1MB 静止 5s 精确送达 **1048576 字节**，sha256 与源一致。
- e2e case25：10MB 粘贴期间键入 `KEYMARK` 不丢，最终 **10485767 字节**（= 10485760 + 7）且含 KEYMARK。
- tag `phase2d-t2`。

## 3. 主字体加载耗时评估（任务 3，只评估）

**实测（release，空闲 Xvfb 1280x720，各 5 次）**

| 字体 | 大小 | fontdue from_bytes 中位（5 次） | app 首帧中位（APP_FONT_FAMILY） |
|---|---|---|---|
| DejaVu Sans Mono | 0.3MB | 11.8ms（11.7–19.0） | 191ms（161–196） |
| Microsoft YaHei | 16.2MB | 230.7ms（228.5–354.4） | 464ms（444–487） |
| SimSun | 17.4MB | 215.2ms（214.7–222.3） | 未测首帧 |

首帧差 ≈273ms，与 from_bytes 增量同量级。对比 Windows 报告（YaHei Mono 2071ms vs 默认 319ms，差 1752ms）：Linux 上大字体代价显著更低，但趋势一致。

**方案评估**

| 方案 | 预期收益 | 实现代价 | 风险 | 结论 |
|---|---|---|---|---|
| a 主字体延后加载（先用小字体占位出首帧） | 首帧与主字体大小解耦；**原型实测 YaHei 464ms→223ms（-52%）** | render 新增 `replace_primary_fonts` + app 后台线程/通道接线（原型约 60 行） | 占位字体与最终主字体 cell 度量不同（等宽西文 vs CJK advance），到位后需重排网格/修正窗口尺寸，首屏可能有一次布局跳变与闪烁 | 原型已验证（分支 `eval/font-load`，未合入 main、未 push）；**待用户决定是否实施** |
| b 只解析所需字形（ttf-parser 轮廓 + 自光栅化） | from_bytes 由 O(字形数) 降到 O(1) | 需自写轮廓扫描转换/抗锯齿，与现有字形缓存/宽字符缩放/subpixel 重复 | 重复实现成熟能力、渲染质量回归 | **不建议**；不更换光栅化库 |
| c ttc/collection 仅加载所需成员 | 有限（单成员仍约 15MB 解析） | 需自解析 ttc 头挑成员（fontdue 无 collection API） | 收益不足 | 暂不实施 |
| d 配置指引：主字体用小的等宽西文字体，大字体放 fallback | 等同 a 的效果且零代码（2C 实测 linux.toml：首帧 202ms、CJK 后台 695ms 到位） | 0 | 最低 | **推荐默认** |

**提请**：默认采用 d；若产品确需"CJK 字体作主字体"，再决定是否实施 a（需同时处理 cell 度量变化导致的布局跳变）。探针 `crates/render/tests/font_load_time.rs`（`#[ignore]`）可复现上表。tag `phase2d-t3`。

## 4. CPU 渲染帧耗时基线（任务 4）

**基准**：`crates/render/tests/bench_frame.rs`（`#[ignore]`，`cargo test --release -p render --test bench_frame -- --ignored --nocapture`），合成满屏快照（ASCII + 汉字宽字符 + 彩色/样式 + 选区 + 滚动条），预热 20 帧、采样 200 帧。帧缓冲 `Vec<u32>`（0xFFRRGGBB）与 softbuffer 格式一致，render 内无额外格式转换。

| 配置 | 全帧热 中位 | p95 | max | 冷首帧 | 单格变化 中位 | 占 60Hz 预算 |
|---|---|---|---|---|---|---|
| 120x30@1x（1080×510） | 6.44ms | 6.75ms | 7.64ms | 7.69ms | 6.48ms | 38.6% |
| 120x30@2x（2040×990） | 21.41ms | 22.43ms | 23.33ms | 22.54ms | 21.74ms | 128% |
| 200x60@1x（1800×1020） | 22.44ms | 23.10ms | 33.09ms | 24.64ms | 22.65ms | 134% |
| 200x60@2x（3400×1980） | 73.02ms | 73.88ms | 75.00ms | 72.55ms | 73.38ms | 438% |

结论（已验证/工程判断）：
- 120x30@1x 可稳定 60fps（38.6% 预算）；**HiDPI 或大网格下 CPU 渲染超 60Hz 预算**：120x30@2x、200x60@1x 约 1.3 倍预算（30Hz 勉强可行），200x60@2x 约 4.4 倍预算。
- 成本随像素量近似线性：200x60@1x（1.8Mpx）22.44ms ≈ 120x30@2x（2.0Mpx）21.41ms；缩放 2.0 使像素 ×4、耗时约 ×3.3。
- **当前无脏区增量渲染**：单格变化帧耗时与全帧基本相同（6.48 vs 6.44、21.74 vs 21.41、22.65 vs 22.44、73.38 vs 73.02），实现是整帧重画（每帧重建 framebuffer、重绘全部格子）。这是阶段 3 决策的输入。
- 冷/热差很小（120x30@1x 7.69 vs 6.44ms），说明字形缓存命中率对静态画面影响有限，主要成本是像素填充。

tag `phase2d-t4`（随后 `e2d4480` 为 clippy 修复：`bench_frame` 的 `%` 改 `is_multiple_of`，数据不受影响）。

## 5. 未解决问题与偏差

- 主字体延后加载（方案 a）只在本地分支 `eval/font-load` 验证，未合入 main；其 cell 度量变化导致的布局跳变未解决（原型未做 set_inner_size 修正，仅替换字体+清缓存）。
- 粘贴排空为"预算化分帧写"：极端慢速对端（不读 PTY）时，单块 64KB 的 `write` 仍是阻塞调用，主线程可能被阻塞一次写入的时长；改变这一点需要非阻塞 fd（拿不到 fd，不改 Pty trait 即无法做）或写线程（属 trait 变更，否决）。10MB/1MB 场景实测无卡顿。
- CPU 渲染整帧重画，无脏区优化；200x60@2x 超 60Hz 预算 4 倍以上，属架构级限制，留待阶段 3（GPU）决策。
- 大字体首帧在 Linux 的代价（+273ms）显著低于 Windows 报告（+1752ms），原因未深究（字体文件实现、体积、文件系统差异），标注为未核实差异。

## 6. 给 Windows 侧的交接（合并后需复验）

1. `clipboard.rs` 合并结果：`primary_get/primary_set` 已改为 `&mut` 接收者（Linux 扩展 API 需要）；Windows 走 `cfg(not(unix))` 分支仍返 Err，请复验 Copy/Paste 行为未变（`PHASE2B-WIN.md` 记录的"Windows 无 PRIMARY"语义不变）。
2. 粘贴排空修复对 Windows 生效：`about_to_wait` 预算排空 + `request_redraw` 自驱动是平台无关代码；请在 Windows 复跑"1MB 粘贴静止场景"与"10MB 粘贴期间输入不丢"，确认原症状（约 2 块后停住）消失。
3. Windows 侧 ConPTY 可能返回 `WouldBlock`：`drain_pending` 已覆盖该分支（保留剩余、下一轮重试、不 spin），请确认大段粘贴下无空转与卡死。
4. e2e 窗口查找已改为按 PID（`xdotool search --pid`）——仅 Linux 脚本；Windows 侧不受影响。
5. tag `phase2d-t1..t4` 之后 main 含 merge 提交与后续修复，Windows 分支若继续开发需先 rebase/merge 到 main（由 Windows 侧决定）。

## 7. 手动清单（≤3，Xvfb/脚本无法覆盖）

1. 真实输入法下的候选窗跟随与预编辑观感（Phase 2C 遗留，仍待真机）。
2. 真实 HiDPI 屏幕上的渲染流畅度（本基准给出耗时，但人眼 60Hz 感知需真机）。
3. 大字体作主字体时延后加载的布局跳变是否可接受（需要人眼评估首屏一次跳变的影响）。

## 8. 验收

- `cargo test --workspace`：90 passed / 0 failed
- `cargo clippy --workspace --all-targets -- -D warnings`：0
- `cargo build --release`：通过
- `scripts/e2e/run_all.sh`：78/78 PASS，连续 3 次全过
