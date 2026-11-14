# PHASE4M-LINUX：合并 Windows 3J / 小粘贴回底 / 字体组微秒打点

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3j-win-done` | 已合并 `eb2b8ae`（tag `phase4m-t1`），共用代码零改动 |
| t2 小粘贴不回底 | 确认 `<1MB` 路径缺回底调用；收敛到粘贴唯一入口（tag `phase4m-t2`） |
| t3 字体组微秒打点 | 新增 `font_steps_us` / `font_group_us`；各步之和与组总时长差 139–280µs（tag `phase4m-t3`） |

## 1. t1 合并证据（ls-remote 实测）

```
79588161d4465af8513ad0e15fdab40e4f04c1ec	refs/heads/main（合并前）
5ffd6faf55b62c45cd398615444f069aa3ff2926	refs/heads/win
23c40713862596c5232cd1c432b4206b7e239d12	refs/tags/phase3j-win-done
eb2b8aef88774625e99573ee971392b8b5b507d2	refs/tags/phase3j-win-done^{}
```

- tag 是附注标签，`^{}` = `eb2b8ae`；`refs/heads/win` = `5ffd6fa`，**比解引用目标多一个提交**：
  `5ffd6fa 3J-win: record push/tag hashes in the report`，仅改 `PHASE3J-WIN.md`（+10 行）。
- 按任务要求合并**解引用后的** `eb2b8ae`。报告里出现过的两个 hash 由此解释：
  `eb2b8ae` = 3J 的工作提交（tag 指向它），`5ffd6fa` = 之后补记 hash 的提交（win 头）。
- 合并相对 main 的共用/受控范围：`.gitignore` 一行（`/vt-verify-win/osc/*.txt`）、
  `PHASE3J-WIN.md`、`docs/WIN-NOTES.md`；**`crates/` 零差异**（Windows 本轮无共用代码改动）。
- 合并后 `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 通过、
  依赖树 `wgpu`/`pollster` 命中 0。

## 2. t2 小粘贴不回底

### 2.1 路径审计（全部"用户输入写 PTY"的路径）

| 路径 | 位置 | 是否回底 |
|---|---|---|
| 可打印字符 `KeyRoute::Text` | `window_event` 键盘分支 | ✅ `follow(Printable)` |
| 编码按键 `KeyRoute::Encoded`（Enter/Backspace/Tab/方向键…） | 同上 | ✅ `follow(EncodedKey)` |
| IME 上屏 `Ime::Commit` | `on_ime_event` | ✅ `follow(ImeCommit)` |
| 粘贴 `≥1MB` 分块 | `paste_from_text` | ✅ 有（本次收敛到唯一入口） |
| 粘贴 `<1MB` | `paste_text_accepted` | ❌ **缺失（本次修复）** |
| 鼠标上报序列 | `on_mouse_input` / `on_cursor_moved` / `on_mouse_scroll` | 按规格不回底 ✅ |
| vt 应答字节（DSR/DA 等） | `pty_write_rx` 排空处 | 非用户输入，不回底 ✅ |

根因：4L t1 重构时把回底调用留在了随后被删除的 `paste_text` 里，`paste_text_accepted`
（`<1MB`）没有补上；`≥1MB` 分块路径仍在 `paste_from_text` 内单独调用，于是只有大粘贴回底。

### 2.2 修复（收敛到单一入口）

- 新增 `AppHandler::paste_accepted(norm: &str)`：**粘贴唯一入口**。回底判定用纯函数
  `follow::paste_returns_to_bottom(len, accepted, alt_screen, mouse_reporting)`
  （＝`accepted && len>0 && should_follow(Paste, …)`），判定在**入队之前**完成一次；
  之后按 `≥1MB` 分块 / `<1MB` 有界入队分流（行为不变）。
- `paste_from_text`：先做积压判定；**被整条拒绝时直接返回**（没有输入发往 PTY → 视口不动），
  通过则整体交给 `paste_accepted`。
- `should_follow` 的纯决策语义未改动；备用屏与鼠标上报模式仍不回底。

### 2.3 门禁与证伪

`case47`（release，cpu 与 gpu 各一轮）：上滚后分别粘贴 **20B / 4KB / 921600B / 1258291B**，
每次断言滚动条拇指回到 ≥0.98；随后再粘贴 **13MiB**（超 12MiB 上限）并断言
`paste_rejected` 出现且**拇指不动**（视口不变）。

| 实现 | 结果 |
|---|---|
| 修复前 | `20b=0.948 4k=0.948 900k=0.952 1200k=0.881` → **FAIL**（小粘贴全红） |
| 修复后 | `20b=1.000 4k=1.000 900k=1.000 1200k=1.000`，拒绝 `0.958→0.958` → **PASS**（cpu/gpu 均 2/2） |
| 证伪（临时关掉回底调用） | `FAIL 未回底: 20b 4k 900k 1200k` → 红；恢复即绿 |

单测：`follow::paste_returns_to_bottom` 覆盖 接受/拒绝/空/备用屏/鼠标上报 五种情形。

## 3. t3 字体组内部微秒打点

### 3.1 新增观测

- `font_steps_us`（每次**缓存未命中**打印一行）：
  `kind=file|binary|shared`、`bytes`、`meta_len`（stat 长度）、`ttc`（扩展名或 `ttcf` 魔数）、
  `query_us`、`face_us`（`db.face`）、`meta_us`（`fs::metadata`）、`read_us`、`settings_us`
  （`FontSettings::default()` 构造）、`parse_us`（`Font::from_bytes` 调用）、`clone_us`、
  `insert_us`、`steps_us`。
- `font_group_us`：`pre_load_us`（组起点→调用加载器）、`load_us`（加载器调用本身）、
  `tail_us`（返回→组尾）、`total_us`——正好对应 Windows 报告里 begin→seg / seg→cache /
  cache→end 三段。
- 仍保留 `font_group_begin_ms` / `font_group_ms` / `font_ms`（毫秒级，历史对比用）。

### 3.2 Linux 分段（release，cpu/gpu 各 5 次，中位，微秒）

| group | `steps_us` | 三段和 | `font_group_us` | 差 |
|---|---|---|---|---|
| 0 Regular（361228B） | 28298 | 28437 | 28438 | 140 |
| 1 Bold（184480B） | 15478 | 15617 | 15617 | 139 |
| 2 Italic（225128B） | 17213 | 17353 | 17354 | 141 |
| 3 Bold Italic（190384B） | 14533 | 14672 | 14673 | 140 |

判据 **≤1ms** 通过（实际 139–141µs，gpu 同量级；gpu group3 一次 280µs）。
固定 ~140µs 的差 = `font_steps_us` 这一行 eprintln 自身耗时。

组 0 的构成：`parse_us=21846`（77%）、`clone_us=6201`（22%）、`read_us=218`、`meta_us=3`、
`query_us=81`、`face_us=0`、`settings_us=0`、`insert_us=1`。全部 `ttc=false`、`kind=file`。

### 3.3 Windows 侧应关注的点与判据

1. **`clone_us`**：Linux 上 361KB 的 `Font::clone` 要 6.2ms（占 22%）。若 Windows 的
   `clone_us` 显著更大（例如 ≥50ms），说明缺口在克隆而非解析——可直接改成
   `cache.insert(id, font.clone())` 之外的一次性 `Arc`/延迟克隆（属优化，本轮不做）。
2. **`meta_us` vs `read_us`**：`meta_us` 大 ⇒ 文件打开/属性被拦（安全软件实时扫描的典型特征，
   因为 stat 也走文件系统过滤驱动）；`read_us` 大而 `meta_us` 小 ⇒ 顺序读盘/缓存未命中。
   两者都小 ⇒ 不是文件系统。
3. **`parse_us` 与 `ttc` 标记**：若 `ttc=true` 且 `parse_us` 远大于 Linux 的 ~22ms，则是
   fontdue 对集合的解析开销。
4. **`scripts_us` 之外的 `tail_us`**：若 `tail_us` 在 Windows 上仍是几十毫秒，说明缺口在
   "打印本身"或组尾逻辑，而不是任何被测量的语句。
5. 判据总述：`font_steps_us`（各语句之和）与 `font_group_us` 的差即"未被语句覆盖的部分"；
   Linux 上该差 139µs ≈ 一行 eprintln。**若 Windows 上这个差达到 10ms 量级，则主因是打印/日志
   I/O 而非字体处理**（3J 的 74/59/32ms 正是两条打印之间的墙钟差）。

## 4. 未解决问题

1. **1200k 档修复前的 0.881 未能解释**（待验证假设，置信度低）：`≥1MB` 路径当时已有 follow
   调用却出现未回底；修复（改为"先回底再入队"）后稳定 1.000。推翻条件见 DECISIONS。
2. **Windows 的三段缺口仍待数据**：本轮只提供 µs 级点，需 Windows 侧复取（重点 `clone_us`、
   `meta_us`、`tail_us`）。
3. **case47 依赖 13MiB 剪贴板**：xclip 读大文件需要 ~1.2s 才取得选择所有权，已在该档加长等待；
   机器更慢时仍可能不足（可加大 sleep，但不改判据）。

## 5. Windows 交接

1. 用本轮新增点复取字体组数据：`font_steps_us` 一行给全部分步，`font_group_us` 给三段。
   重点看 `clone_us`、`meta_us`、`read_us`、`ttc`、以及 `font_group_us.total_us - font_steps_us.steps_us`。
2. t2 的修复在 Windows 同样生效（纯 app 层）：小粘贴应回底；`Ctrl+Shift+V`/右键一致。
   若仍不回底，检查 `is_alternate_screen`/`is_mouse_tracking` 是否为真（这两个条件按规格抑制回底）。
3. 13MiB 被拒时的语义与 Linux 一致：`paste_rejected oversized=true`，视口不动。

## 6. 手动清单（≤3）

1. 上滚到历史后分别粘贴一小段与一大段文本，确认两者都回到最新输出（本次修复的用户可见点）。
2. Windows 侧复取字体组 µs 分段，按 §3.3 的判据区分"文件系统/安全软件"与"解析"。
3. 若 Windows 上 `font_group_us` 与 `steps_us` 仍差数毫秒以上，考虑把启动期打点改为写文件
   （避免控制台写放大），但这属优化，需另行确认。
