# PHASE4H-LINUX：Windows 3F-b 合并 / 启动打点补全 / PTY 写阻塞与写线程 / 反色语义

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3f-b-win-done` | 已合并（tag `phase4h-t1`），共用代码零改动 |
| t2 启动打点补全 | 已补 11 类点，未标注残留 ≤1.5ms（tag `phase4h-t2`） |
| t3 PTY 写阻塞 | **阻塞成立（12.9s）**，已实施独立写线程（tag `phase4h-t3`） |
| t4 反色语义回归 | 4 例通过，Linux CPU/GPU 均符合预期（tag `phase4h-t4`） |

## 1. t1 合并证据

`git ls-remote ali`（合并前实测）：

```
3bcb1f770782d3ff92cff34b76a8e5d50d4275af	refs/heads/main
62a8d5acc18f32f8948477aa89bf52caee706ff8	refs/heads/win
41cb3aa4c284637db019db92b0f8ff6ba82366ec	refs/tags/phase3f-b-win-done
62a8d5acc18f32f8948477aa89bf52caee706ff8	refs/tags/phase3f-b-win-done^{}
```

即 tag 是附注标签，指向 commit `62a8d5a`（= win 头）。

合并基 `25f8026`，win 侧 1 个提交、53 个文件、+1277/−142。**共用范围仅 3 处**：

| 文件 | 变更 |
|---|---|
| `.gitignore` | 新增 `/vt-verify-win/probe_inv/target/` |
| `PHASE3F-B-WIN.md` | 新增 Windows 报告 |
| `docs/WIN-NOTES.md` | Windows 专属文档更新 |

其余 50 个文件全在 `vt-verify-win/`、`scripts/windows/`，无共用代码改动 → `--no-ff` 合并，合并后 144 测试 0 失败、e2e 93/93。

## 2. t2 启动分段打点（只补观测，无优化）

### 2.1 新增打点

| 层 | 打点 | 覆盖的区间 |
|---|---|---|
| render | `cpu_new_begin_ms` | 进入 `CpuRenderer::new` 的时刻 |
| render | `font_lookup_begin_ms` | 每组字体的查找入口（此前该段无标注） |
| render | `font_seg_ms family/style/bytes/query_us/read_ms/parse_ms` | 字体解析三段拆开 |
| render | `font_cache_ms` | `Font` clone + 缓存写入 |
| render | `cpu_groups_ms` / `cpu_state_ms` | 字体链构建总时长 / 结构初始化 |
| render | `gpu_begin_ms` / `gpu_state_ms` | GPU 侧入口与收尾（instance/adapter/device/atlas/surface/pipeline 已有） |
| app | `app_new_ms` / `event_loop_ms` / `pre_window_ms` | 配置归一化 / X11 连接 / 窗口属性组装 |
| app | `ime_ms` / `renderer_build_ms` / `vt_new_ms` | 窗口后到 renderer_ready、VtTerminal 创建 |
| app | `renderer_ready_ms` | 由整数毫秒改为 1 位小数（消除求和 ±1ms 截断） |

全部 release 可用、只在启动路径打印一次（约 30 行 stderr）、无堆分配、热路径零影响。

### 2.2 覆盖核对（:99 release，cpu/gpu 各 5 次）

判据：相邻打点间隙减去"覆盖该间隙的实测值"后的残留 ≤30ms。

| 后端 | 残留中位 | 残留最大 | 结论 |
|---|---|---|---|
| cpu | 0.9ms | 1.0ms | 通过 |
| gpu | 1.3ms | 1.5ms | 通过 |

### 2.3 分段表（since_start 中位，ms）

| 打点 | cpu | gpu |
|---|---|---|
| main_ms | 0.0 | 0.0 |
| config_ms | 0.3 | 0.3 |
| app_new_ms | 0.3 | 0.3 |
| event_loop_ms | 27.4 | 38.1 |
| cpu_new_begin_ms | 27.4 | 38.2 |
| fontdb_ms | 98.1 | 114.9 |
| font_seg(Regular) | 120.9 | 137.1 |
| cpu_groups_ms | 178.9 | 194.1 |
| cpu_state_ms | 178.9 | 194.2 |
| window_ms | 181.4 | 196.1 |
| ime_ms | 181.4 | 196.1 |
| renderer_ready_ms | 181.5 | 377.0 |
| gpu_begin_ms | — | 196.1 |
| gpu_instance_ms | — | 264.2 |
| gpu_device_ms | — | 349.1 |
| gpu_surface_ms | — | 370.8 |
| gpu_pipeline_ms | — | 376.9 |
| surface_ms | 182.0 | 377.1 |
| vt_new_ms | 182.1 | 377.2 |
| pty_spawn_ms | 185.4 | 382.9 |
| pre_first_submit_ms | 190.0 | 385.0 |
| first_frame_ms | 202.0 | 417.0 |

### 2.4 哪些点会在 Windows 上提供新信息

1. `fontdb_ms` / `font_seg_ms`（含 `read_ms`、`parse_ms` 拆分）/ `font_cache_ms`——
   Windows 首帧 781ms 中"字体加载结束到 cpu_renderer_ready 约 362ms"这段此前无打点；
   这三类点可直接把 362ms 拆成"系统字体库扫描 / 文件读取 / fontdue 解析 / 缓存写入"。
2. `event_loop_ms` / `pre_window_ms`——ConPTY + Windows 事件循环初始化成本（Linux 27ms，
   Windows 预计明显更大，属关键对比项）。
3. `ime_ms` / `renderer_build_ms` / `vt_new_ms`——"窗口创建完成到 renderer_ready 约 120ms"
   的可归因分段。
4. `gpu_*` 系列在 Windows 上不适用（GPU 后端仅 Linux 编译）。

## 3. t3 PTY 写阻塞：成立，已实施写线程

### 3.1 4G 结论作废（已验证事实）

`APP_TEST_PASTE` 是 `#[cfg(debug_assertions)]` 钩子——4G 用 **release** 跑它**根本没触发粘贴**，
测到的 90–135ms 是事件循环空闲间隔。本轮改用真实粘贴路径（xclip + 右键 → `do_paste` →
`paste_from_text`）在 release 复测。

### 3.2 场景 A/B 量化（release，:99，每格 3 次）

| 场景 | 1MB(780000B) | 5MB(5070000B) |
|---|---|---|
| A 规范模式 `sleep 15` | 12953 / 12936 ms | 12936 / 12894 / 12886 ms |
| B 非规范 `stty -icanon -echo; sleep 15` | 12900 / 12893 / 12903 ms | 1303 / 12820 / 12818 ms |

11 次中 10 次 **≈12.9s**（median 12893ms）；停顿长度 = 剩余 sleep 时长，且 1MB 与 5MB
无差别（受 sleep 支配）。

### 3.3 假设证伪（已验证事实）

"规范模式下行缓冲满后内核丢弃多余输入，所以测不出阻塞" **不成立**：sleep 结束后用
`head -c` 读回统计，1MB 场景收到 **778240/780000**（缺 1760B）、5MB 场景收到
**5066752/5070000**（缺 3248B）——字节被缓冲并最终送达，写端是在阻塞等对端消费。

判据（≥100ms 且与数据量相关）成立 → 实施。

### 3.4 实施方案与契约

- **trait**：`Pty` 新增带默认实现的方法
  `fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> { None }`；Unix 返回内部写端，
  **Windows 实现不改**（默认 None → app 回退同步写，行为不变）。现有方法签名与语义零改动。
- **写线程**（`crates/app/src/writer.rs`）：
  - 有界队列 `WRITER_QUEUE_CAP = 1MiB`：取值理由为 `PASTE_CHUNK=64KiB`、单轮排空预算
    512KiB，1MiB ≈ 两轮预算在途，既让写线程常有事做又不把 10MB 堆进内存；**只约束粘贴**，
    键入始终入队（不丢）。
  - 顺序：单队列 + 单消费者，键入与粘贴同序写出。
  - Ctrl-C（0x03）：入队前丢弃队列中**尚未写出**的粘贴项，使中断立即生效（行为决策）。
  - 关闭：`close()` 置 closing → 线程写空队列后 drop 写端（释放 PTY 主端）→ join 超时
    2s 则分离并记录日志 → 随后 `Pty::close()` 回收子进程。
  - 错误：EIO/EPIPE 置 `failed`、清空队列、线程退出，不 panic；app 走既有退出路径。
  - 并发：写端由线程独占；主线程只经 `Mutex<State> + Condvar` 入队。
- **粘贴排空**：新增 `paste::drain_to_writer`，队列满等价 `WouldBlock`（保留剩余、保持
  既有预算化与 `request_redraw` 自驱动）。

### 3.5 门禁证据

| 用例 | 结果 |
|---|---|
| case38 场景 B 粘贴（粘贴窗口内每 40ms 驱动事件） | `loop_gap_max=92ms`（<100）；缩放 1040→720 生效；Ctrl-C 后提示符返回、1MB 末行 `paste38_line_019999` 未出现、画面静止 |
| case38 回退验证（`take_writer` 返回 None） | `gap=NA`（主线程被阻塞到无法上报）→ **红** ✅ |
| case39 正常读取 shell 粘贴 7.28MB（关回显） | 收到 **7280000/7280000** 字节，`gap=7ms` |
| case40 粘贴中关闭窗口 | 250ms 退出、无 panic、无残留 shell 子进程 |
| case25 / case32 / case35 | 全过 |
| 单测 | 新增 7 例：队列上限、Ctrl-C 丢弃、顺序、写端错误不 panic、close 排空、`drain_to_writer` 满/空两例 |

**测量方法修正（已验证事实）**：`loop_gap` 在静止画面下记录的是空闲间隔（首次测得
1186ms 假停顿＝等定时唤醒）。因此钩子触发粘贴时重置统计、门禁在粘贴窗口持续驱动
鼠标事件、且只统计 `test_paste_fired` 之后的日志。

### 3.6 顺带修复

`APP_TEST_PASTE` 钩子到期时未保持 `WaitUntil` 唤醒：渲染路径"到期渲染"后不再设置
ControlFlow，事件循环进入无限 Wait，导致静止场景下 `APP_TEST_PASTE_AT_MS` 永不触发
（此前所有依赖该钩子的静止用例都有此隐患）。已在 `about_to_wait` 末尾补定时唤醒。

## 4. t4 反色与默认色语义

新增 `crates/render/tests/inverse_default_color.rs`（只加测试，未改渲染代码），
调色板刻意取 `primary.foreground=#112233`、`normal[7]=#ABCDEF`：

| 输入 | 期望 | 实测 CPU / GPU |
|---|---|---|
| `ESC[37m ESC[7m ` + 空格 | 底色 = normal[7] | #ABCDEF / #ABCDEF ✅ |
| `ESC[7m ` + 空格 | 底色 = primary.foreground | #112233 / #112233 ✅ |

现有行为与预期相符，未触发停止条件。Windows 3F-b 观察到的"默认反色块底色为
normal.white"据此定性为 **ConPTY 把默认前景改写成显式 `ESC[37m`** 的平台行为
（待验证假设，置信度中）。

## 5. 未解决问题

1. **Windows 的 362ms 归属仍未核实**：本轮只提供打点，Windows 侧需重新取数。
2. **Ctrl-C 与已写入字节**：`take_writer` 只能丢弃"队列中未写"的部分；已写入 tty 输入
   队列的字节（约 ≤64KiB 一块）仍会被 shell 当作命令执行（实测 case38 中 shell 执行了若干
   `paste38_line_0000xx`）。若要求"中断后这些也不执行"，需要改用 TIOCSTI 清理或加应用层
   粘贴事务，均超出本任务范围。
3. **回显背压**（已验证事实）：7MB 粘贴在**回显开启**时因 tty 把每个字节回显回 app 读端，
   实测只送达 4.19MB 且 loop_gap 21s。这与写阻塞无关，但说明"大粘贴 + echo on"本身有
   吞吐瓶颈；case39 以 `stty -echo` 规避。是否在 app 侧对大粘贴自动关闭回显，未做决策。
4. **`loop_gap` 语义边界**：它无法区分"主线程被阻塞"与"主线程忙于处理"（后者在大回显
   场景同样产生长间隔）。

## 6. Windows 3H 交接

1. 用本轮新增打点在 Windows 复取首帧：重点看 `fontdb_ms`、4 组 `font_seg_ms`
   （`read_ms` vs `parse_ms`）、`font_cache_ms`、`event_loop_ms`、`ime_ms`、
   `renderer_build_ms`、`vt_new_ms`。
2. 反色语义：`cargo test -p render --test inverse_default_color` 应通过；若 Windows 上
   "默认反色"用例失败（取到 normal[7]），说明 ConPTY 确实改写了默认前景——此时**不要改
   渲染代码**，应在报告中登记为平台行为，并复核 3F-b 的判定是否需要修正。
3. 写线程：Windows 的 `take_writer` 走默认实现返回 `None`，仍为同步写。若 Windows 侧
   出现粘贴卡顿，先按 t3a 方式量化（`APP_PERF_FRAMES` 的 `loop_gap_ms` + 粘贴窗口内驱动
   事件），达 ≥100ms 再决定是否实现 ConPTY 的 `take_writer`。
4. `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 本轮通过。

## 7. 手动清单（≤3）

1. office-pc 上粘贴 10MB 到正在编译/忙的窗口，确认界面不再卡死（本轮 Linux 已从 12.9s 降到 <100ms）。
2. Windows 侧复取首帧分段（命令见 6.1），确认 362ms 的具体构成。
3. 备用屏 TUI（vim/less）中做大粘贴，确认写线程不与全屏刷新交互异常（本轮只用单元测试与
   shell 场景覆盖，未做 TUI 目视）。
