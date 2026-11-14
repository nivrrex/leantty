# PHASE4L-LINUX：粘贴积压语义修正 / tag 机械化防护

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 粘贴积压超限语义 | 取证确认"截断尾部"会切掉 `ESC[201~` 并切开 UTF-8；改为**整条拒绝**（tag `phase4l-t1`） |
| t2 tag 机械化防护 | `scripts/gates.sh` + `scripts/tag_checked.sh` + 自测 8/8（tag `phase4l-t2`，且由 `tag_checked.sh` 打出） |

## 1. t1a 取证

### 1.1 实际次序（读代码得到）

```
do_paste → paste_from_text(text)
   ① bracketed = vt.is_bracketed_paste()
   ② norm = normalize_paste(text, bracketed)        ← 先加 ESC[200~ … ESC[201~
   ③ 旧实现：accept_within_backlog(bytes, backlog, cap)  ← 从**尾部**截断
   ④ 入队（<1MB → paste_text 的有界入队；≥1MB → pending_paste 分块）
```

所以**包裹在截断之前**，而截断从尾部切 → 结束标记必然落在被切掉的部分里；
≥1MB 路径还有 `String::from_utf8_lossy`，会把被切开的半个字符替换成 U+FFFD。

### 1.2 纯函数取证（结构性，已验证事实）

输入 `"中"×20` 走括号包裹后 72 字节、尾部为 `\x1b[201~`；只给一半空间（奇数长度）：

```
接受 len=37 丢弃=35 尾部=[228, 184, 173, 228]
以 ESC[201~ 结尾: false
合法 UTF-8: false
lossy 引入 U+FFFD: true
```

→ 结束标记丢失 + 半个 UTF-8 字符，两个担忧都成立。

### 1.3 真实路径复现（`scripts/e2e/paste_backlog_probe.sh`）

流程：bash --norc（readline 开启括号粘贴）→ 前台 `sleep 25` 不读 stdin → 单条 13MiB
真实粘贴（xclip + 右键，> 12MiB 上限）→ sleep 结束后按 Return、再键入
`echo WEDGE_OK_$((1+1)) > 标记文件` → 检查标记内容 `WEDGE_OK_2`。

> 为什么用"命令是否执行"而不是屏幕文字：bash 的括号粘贴把文本插入**行缓冲**，按 Return
> 才执行；若结束标记丢失，readline 停在"粘贴中"，Return 与命令都被当字面内容插入。

旧实现 5 次结果：

| 轮 | 首次尝试 | +20s 重试 | Ctrl-C 后 |
|---|---|---|---|
| 1 | 未执行 | 未执行 | **执行（WEDGE_OK_2）** |
| 2 | 未执行 | 未执行 | **执行** |
| 3 | 未执行 | 未执行 | **执行** |
| 4 | 未执行 | 未执行 | **执行** |
| 5 | 未执行 | 未执行 | 仍未执行 |

即 **5 次中 4 次必须 Ctrl-C 才恢复、1 次连 Ctrl-C 也没恢复**（每次都有 1 条 `paste_dropped`）。

## 2. t1b 规格（已写入 DECISIONS）

积压上限触发时，新到达的粘贴**整条拒绝**：一个字节都不写、不入 pending；日志
`[perf] paste_rejected bytes=N backlog=B cap=C oversized=<bool>`；已在途的粘贴不受影响；
单条粘贴本身 >12MiB 同样整条拒绝并用 `oversized=true` 标出。不新增配置项、不新增 UI。

## 3. t1c 实现（最小改动）

- `paste::accept_within_backlog(&[u8], usize, usize) -> (&[u8], usize)` → 改为全有或全无
  `paste::accepts_whole(bytes_len, backlog, cap) -> bool`。
- 新增唯一判定入口 `AppHandler::accept_paste_or_reject(bytes_len) -> bool`：在包裹**之后**、
  入队**之前**调用；拒绝即返回（不产生任何写入），通过则整段（含结束标记）入队。
- `<1MB` 与 `≥1MB` 两条路径都先过一次该入口；入队逻辑抽为 `paste_text_accepted`
  （原 `paste_text` 因重复判定而同名合并，删除重复判定路径）。
- 单测 5 条：恰好等于上限（接受）、差 1 字节（拒绝）、积压已满（拒绝）、单条超上限（拒绝）、
  括号包裹的整条拒绝不产生中间态。

## 4. t1d 门禁与证伪

`case46`（release，cpu 3 轮 + gpu 2 轮 = 5/5）：慢消费者 + 连续 1MB 粘贴直至出现
`paste_rejected`，随后用户 Ctrl-C 取消，再键入 `echo WEDGE_OK_$((1+1)) > 标记`。

| 实现 | 结果 |
|---|---|
| 新实现（整条拒绝） | **5/5 PASS**（`rejected=1 dropped=0 marker=WEDGE_OK_2`） |
| 旧实现（`git stash` 掉修复） | **5/5 FAIL**（`rejected=0 dropped=1`） |

**证伪的限定说明（已验证事实）**：case46 里用户的 Ctrl-C 会治愈楔死，因此它的 marker
断言在新旧实现下都能通过；真正区分两者的是日志语义（`paste_rejected` vs `paste_dropped`）。
**楔死本身**由 §1.3 的 `paste_backlog_probe.sh` 单独证明（4/5 需 Ctrl-C、1/5 不可恢复）。

顺带修复：`mem_summary` 的 `RendererBackend::Gpu` 分支未按平台门控，导致
`cargo check --target x86_64-pc-windows-gnu --workspace --tests` 编译失败（4K 期间即已存在，
本轮首次真正复现），已加 `#[cfg(target_os = "linux")]`。

## 5. t2 tag 机械化防护

### 5.1 `scripts/gates.sh`

依次执行：`cargo test --workspace` → `clippy`（debug，`-D warnings`）→ `clippy`（release，
`-D warnings`）→ `cargo build --release`（统计 `^warning` 必须为 0）→ `scripts/e2e/run_all.sh`
→ Windows 依赖树 `wgpu|pollster` 命中必须为 0 → `cargo check --target x86_64-pc-windows-gnu
--workspace --tests`。要求起始工作区干净、结束时再次干净；全过则把 `hash=<HEAD>` 与
`time=<UTC>` 写入 `<git-dir>/gates_ok`，任一失败即删除该文件并非零退出。

### 5.2 `scripts/tag_checked.sh <名>`

仅当 ①工作区干净 ②`gates_ok` 存在且其 hash **等于当前 HEAD** ③tag 名未被占用
④名字匹配 `^phase[0-9]+[a-z]*-[A-Za-z0-9._-]+$` 时才 `git tag`；否则打印原因并非零退出。
脚本内无 `-f`/`--force`，不会移动既有 tag。

### 5.3 自测（`scripts/selftest_tag_checked.sh`）

临时仓库（把脚本复制进去并提交，避免"工作区脏"干扰）覆盖 8 条路径，全过：

```
通过：脏工作区被拒绝 / gates_ok 缺失被拒绝 / hash 不匹配被拒绝 / 非法命名被拒绝 /
      成功创建 tag / 重名被拒绝 / 既有 tag 未被移动 / 脚本内无 -f/--force
===== selftest_tag_checked: 通过=8 失败=0 =====
```

### 5.4 本任务起的使用情况

- `phase4l-t2`：由 `sh scripts/tag_checked.sh phase4l-t2` 打出（门禁 hash = `e3ff842`）。
- `phase4l-done`：同样经 `tag_checked.sh` 打出（见 §7）。
- 规则已写入 `docs/DECISIONS.md` 与仓库 `AGENTS.md`：**此后打 tag 一律用 `tag_checked.sh`**。

### 5.5 门禁过程中遇到的一个既有 flake（已验证事实）

`case21-autoscroll` 在前两次 `gates.sh` 运行中失败（`仅 27 行`，并伴随
`X Error ... BadWindow (invalid Window parameter)`），第三次运行通过（104/104）。
隔离验证：带上 run_all 的 `APP_CONFIG`（backend=cpu）单跑 case21 **3/3 通过**（39–41 行）；
不带配置（默认 auto→GPU 路径）时稳定 27 行不达标。故该用例对"渲染后端/时序"敏感，
属既有 flake（本轮未修改该用例，也未放宽其阈值）。

## 6. 未解决问题

1. **case21-autoscroll 的敏感度**（工程判断，置信度中）：GPU 后端路径下拖选自动滚动推进
   的行数不足以越过 `>28` 阈值；run_all 显式用 cpu 后端，故正常路径不受影响，但用例本身的
   阈值/时序仍偏紧。推翻条件：若在干净机器上连续多轮 run_all 稳定通过，则本次失败仅是环境
   抖动而非后端差异。
2. **12MiB 上限仍是产品取舍**：整条拒绝意味着"极端连续大粘贴"时新粘贴会被丢弃（有日志），
   若要无损需背压或磁盘暂存。
3. **`paste_backlog_probe.sh` 未进 run_all**（单轮约 90s，5 轮较慢），目前靠人工/报告期运行。

## 7. Windows 3L 交接

1. 整条拒绝语义与平台无关（纯 app 层），Windows 上 ConPTY 路径同样生效；若 Windows 侧粘贴
   大文本"少了整条"，查是否打了 `[perf] paste_rejected`（属预期行为）。
2. `gates.sh` 的 Windows 检查用 `x86_64-pc-windows-gnu` 目标；Windows 本机可直接跑
   `cargo test --workspace` 等同一组命令（脚本中路径与命令均为仓库相对/通用）。
3. 本轮修的 `mem_summary` 平台门控对所有平台的 Windows 目标编译必需——Windows 侧下次合并
   后应先确认 `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 干净。

## 8. 手动清单（≤3）

1. 在真实终端里对极慢的消费者连续粘贴超大文本，确认出现 `[perf] paste_rejected` 而不是
   半截粘贴（readline 不应卡在粘贴状态）。
2. 下次打 tag 时用 `sh scripts/tag_checked.sh <名>`（先跑 `sh scripts/gates.sh`），确认脚本
   拒绝"未过门禁/哈希不匹配"的情况。
3. 若观察到 case21 在多轮 run_all 中偶发失败，请记录当时是否有其他重负载进程（用于判断是否
   环境抖动）。
