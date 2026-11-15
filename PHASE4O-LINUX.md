# PHASE4O-LINUX：合并 Windows 3L / 测试可移植性 / perf 日志默认关闭 / 后台字体打点

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3l-win-done` | 已合并 `cdc49f`（tag `phase4o-t1`），Windows 侧零共用代码改动 |
| t2 测试可移植性 | 无绝对路径；黄金哈希加**字体指纹**（不符则 SKIP 并打印）；实例数断言改为 fontdb 推定（tag `phase4o-t2`） |
| t3 perf 日志默认关闭 | 默认 stderr `[perf]` 行数 0，`APP_PERF_FRAMES=1` 时 47 行（tag `phase4o-t3`） |
| t4 后台字体打点 | `kind=background` / `cache_hit` / `strong_refs` 全到位，Σ 与总时长差 **−76µs**（tag `phase4o-t4`） |

## 1. t1 合并证据（ls-remote 实测）

```
53db541e27848fe77770dc07a9f1bbf16cf5462f	refs/heads/main（合并前）
65a91d0525f43bbc91c627fee2cbe5edd6fc00a4	refs/heads/win
bfbccef86a54d88801986bc9ebf4957f25d80207	refs/tags/phase3l-win-done
cdc49fca8c5fec2ca23a1d436f8270f34b06817a	refs/tags/phase3l-win-done^{}
```

- tag 为附注标签，解引用 = **cdc49f**；`refs/heads/win` = **65a91d0**，比它多一个
  "record push/tag hashes and commit contents" 提交（仅改 `PHASE3L-WIN.md`，+8/−1）。
  按任务要求合并解引用后的 `cdc49f`。
- **Windows 侧本轮未触及 `crates/`**：用 `git diff-tree` 逐提交核对
  `cdc49fc / 033b6d5 / e172399` 的改动文件数（`crates/` 均 0）。`cdc49f` 祖先里出现的
  `crates/` 改动来自 main 的 4M 提交（2d08837/83dd168/61a02d1），已在 main 中。
- 合并后相对 main 的共用/受控范围：`.gitattributes`（新增 `vt-verify-win/logs/** -text`）、
  `.gitignore`、`PHASE3K-WIN.md`（上一轮漏下的 3K 补记提交带入）、`PHASE3L-WIN.md`、
  `docs/WIN-NOTES.md`；`crates/` 零差异；新增 `scripts/windows/package.ps1` 等。
- 合并后 `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 通过、依赖树
  `wgpu`/`pollster` 命中 0。

## 2. t2 测试可移植性审计

### 2.1 审计表

| 文件 | 字体来源 | 绝对路径 | 写死数值/隐含前提 | 处理 |
|---|---|---|---|---|
| `frame_bytes_identical.rs` | 族名 `DejaVu Sans Mono` + 回退 `Noto Sans CJK SC`（fontdb 解析） | 无 | 黄金哈希 `507485fb7945fb8f` 隐含依赖本机 DejaVu（343140B） | 加**字体指纹**；不符则 SKIP 并打印；支持 `TEST_FONT_FILE` |
| `font_sharing.rs` | 族名 `DejaVu Sans Mono`/`Monospace` | 无 | `唯一实例数 == 4`（假设四样式解析为四个不同 face） | 改为按 fontdb 查询四样式推得的唯一 face 数；族缺失时 SKIP |
| `font_memory.rs` | 族名 `DejaVu Sans Mono`/`Noto Sans CJK SC/JP` | 无 | 无断言（只打印实测值），缺族时已 SKIP | 保持 |

全目录 grep `/usr/`、`/home/`、`/root/`、`C:\` **均无命中**。

### 2.2 指纹机制

`frame_bytes_identical` 先算指纹 `文件名:字节数:前4KB的FNV低16位:cell宽x高`：

- 本机记录值 `DejaVuSansMono.ttf:343140:c69b:8x16` → **真正断言**黄金哈希
  `507485fb7945fb8f`（Linux 基准环境没有被放宽）；
- 指纹不符 → `SKIP: 字体环境指纹不符——实测 X，期望 Y；本机当前哈希 Z`；
- `TEST_FONT_FILE=<路径>` → `SKIP: ... 指纹不可比` 并打印该文件指纹。

实测三条路径：本机=断言通过；改期望值模拟异机=SKIP 并打印；
`TEST_FONT_FILE=LiberationMono-Regular.ttf`=SKIP 并打印 `LiberationMono-Regular.ttf:108172:0fb8:8x16`。

### 2.3 用例数不变

workspace **174 passed / 2 ignored**（render 61 passed），与修复前一致；
`cargo check --target x86_64-pc-windows-gnu --workspace --tests` 通过。

## 3. t3 启动期 `[perf]` 默认关闭

### 3.1 分类（取证）

| 类别 | 例子 | 处理 |
|---|---|---|
| 性能/诊断（受开关） | `main_ms`/`config_ms`/`event_loop_ms`/`fontdb_ms`/`font_steps_us`/`font_group_us`/`bg_*`/`window_ms`/`surface_ms`/`vt_new_ms`/`pty_spawn_ms`/`first_frame_ms`/`bg_fonts_ms`/`gpu_*`/`loop_gap_ms`/测试钩子 | 改走 `eperf!`（`perf_on()` 门控） |
| 面向用户（保持默认可见） | `config: …` 告警、`hinting: fontdue 不支持 hinting`、`request_inner_size: 尺寸请求异步提交`、`[renderer] gpu 不可用，回落 cpu`、`backend=gpu 在当前平台不可用`、各类 IO/surface/pty 错误 | 原样 `eprintln!` |

### 3.2 实现

新增 `perf_on()`（读**已有**环境变量 `APP_PERF_FRAMES`，`OnceLock` 缓存）+ `eperf!` 宏，
分别位于 `crates/render/src/lib.rs`、`crates/render/src/gpu/mod.rs`、`crates/app/src/lib.rs`、
`crates/app/src/main.rs`（宏以 `pub(crate) use` 供同 crate 其它模块使用）。
**未新增环境变量、未新增配置项。**

### 3.3 验证

| 项 | 结果 |
|---|---|
| release 默认启动 stderr `[perf]` 行数 | **0** |
| `APP_PERF_FRAMES=1` | **47 行**（含 `first_frame_ms`） |
| 用户可见警告 | 仍在（实测 `config:`/`hinting:`/`request_inner_size:` 三条照常输出） |
| 新增 e2e `case48` | 断言"默认 0 行 / 开关后 >10 行且含 `first_frame_ms`" → PASS |
| 依赖 `[perf]` 的既有用例 | `case31`（`os_redraw`）、`case32`（`test_paste_first_frame`）、`case47`（`paste_rejected`）补 `APP_PERF_FRAMES=1`；`case38/39/41/43/45/46` 原本已设 |
| e2e 全量 | **106/106**（GPU 轮 10/10） |

### 3.4 启动成本对比（:99 release，`2>文件`，各 5 次）

| 模式 | "进程启动→窗口出现" 5 次（ms） | 中位 |
|---|---|---|
| 默认（无 `[perf]`） | 208 / 203 / 187 / 204 / 197 | **203** |
| `APP_PERF_FRAMES=1` | 193 / 193 / 192 / 198 / 187 | **193** |

差值落在测量噪声内（`[perf]` 共 47 行，按 3L 的 bash 句柄 ~28µs/行估算 ≈1.3ms，低于噪声）；
默认模式无法打印 `first_frame_ms`，故用"窗口出现"这一外部口径比较。**不作承诺。**

## 4. t4 后台回退字体与缓存命中打点

新增（全部受 `APP_PERF_FRAMES` 门控）：

| 点 | 内容 |
|---|---|
| `font_steps_us … kind=background src=…` | 后台回退面复用主字体同一条加载+打点函数（新增 `label` 参数），字段含 `read_us/parse_us/arc_us/clone_us` |
| `bg_fontdb_us` / `bg_db_new_us` | 后台线程内 fontdb 扫描与 `Database::new` |
| `bg_font_clone_us` | `load_font_list` 返回 `Vec<Font>` 造成的**深拷贝** |
| `bg_specs_loop_us` / `bg_generic_loop_us` | 回退面循环 / 系统泛型回退循环 |
| `bg_font_free_us` | 深拷贝后释放原实例 |
| `bg_print_us` | 打点自身开销累计（避免"各步之和"永远小于总时长） |
| `bg_fonts_merge_us … strong_refs=[…]` | 并入四组：实例数与**每个实例的强引用计数** |
| `font_group_us … cache_hit=true\|false` | 缓存命中可见 |

同时删除因本次修改失效的两个旧函数（`load_font_styled` / `load_font_with_fallback`）。

### 4.1 分段表（release，:99，cpu/gpu 各 5 次，中位，µs）

| 项 | CPU | GPU |
|---|---|---|
| 背景面 `parse_us` | 316460 | 329832 |
| 背景面 `read_us` | 2661 | 3222 |
| 背景面 `arc_us` | **0** | **0** |
| 深拷贝 `bg_font_clone_us` | 134772 | 180738 |
| 泛型回退（2 个） | 77221 | 80225 |
| `bg_fontdb_us` | 104536 | 90269 |
| `bg_font_free_us` | 8322 | 10121 |
| `bg_print_us` | 172 | 180 |
| **逐次 Σ 与 `bg_font_list_us` 的差** | **−75…−83（中位 −76）** | **−74…−82（中位 −77）** |
| `bg_fonts_merge_us` / `strong_refs` | 9 / `[5,5,5]` | 13 / `[5,5,5]` |

判据 **≤2ms 通过**（实际 |差| ≤83µs）。
注意口径：`bg_fonts_ms`（本次 824/1056ms）是"自进程启动到后台字体并入"的**绝对时刻**，
不能与各步耗时之和（≈0.64–0.71s）直接相减；可比的量是 `bg_font_list_us`。
另注：**不能先对各分量取中位再相加**——那样会得到毫秒级假差（本轮先踩后纠）。

### 4.2 与 4N 前的对照

- `arc_us = 0`：4N 之后"替换 clone 的那一步"已是 `Arc::new`，实测 0–1µs（4N 前该步是
  100% 深拷贝，CJK 面 ≈300MB）。
- `strong_refs=[5,5,5]`：每个回退面 **1 份实例 + 4 个样式组共享**（5 = 1 本地 + 4 组），
  证明 4N 的共享确实生效。
- **仍未优化**：`load_font_list` 返回 `Vec<Font>`，每个回退面仍有一次深拷贝
  （CJK 135–181ms）并随后释放原实例（8–10ms）。这是后台路径最大的单项成本，
  要消除它得把 `load_font_list` 的返回类型改成 `Vec<Arc<Font>>`（公共 API 变更，
  本轮按"不做优化"未动）。

### 4.3 Windows 侧应关注

1. `parse_us`（回退面解析）：Windows 3L 的 parse 明显更大时，`ttc=true` 与 `kind=background` 可区分集合解析。
2. `bg_font_clone_us`（深拷贝）：YaHei 级别字体在 Windows 上若出现数百毫秒级，优先改为共享返回。
3. `bg_fontdb_us`：Windows 系统字体库扫描通常更慢，是后台路径的固定开销。
4. `bg_font_free_us`：深拷贝后释放原实例的成本，Windows 上可能更贵（大块内存回收）。
5. `strong_refs`：若不为 `[5,5,5]`（例如 `[1,1,1]`），说明共享失效、回到按组深拷贝。

## 5. 未解决问题

1. **`load_font_list` 仍深拷贝一次**（135–181ms/个 CJK 面）：属未做的优化，需 API 变更与另行评估。
2. **默认模式的启动成本对比只能用"窗口出现"作代理**：`first_frame_ms` 默认不打印，
   差值落在噪声内，故本轮不断言 `[perf]` 打印带来的启动差。
3. **`TEST_FONT_FILE` 只影响指纹、不影响渲染器实际加载的字体**：因为渲染器按族名解析，
   要让测试真正换字体需改产品接口（未做），当前 SKIP 行为已如实打印。

## 6. Windows 交接

1. 合并后先跑 `cargo check --target x86_64-pc-windows-gnu --workspace --tests`（本轮通过）。
2. 默认运行不再有 `[perf]` 输出：Windows 侧脚本如依赖这些行，请显式设 `APP_PERF_FRAMES=1`
   （3K/3L 的 `run_3k_*`/`run_3l_*` 属 Windows 专属脚本，需自行加）。
3. 新增点可在 Windows 直接取数：`font_steps_us kind=background`、`bg_font_clone_us`、
   `bg_fontdb_us`、`bg_font_free_us`、`bg_fonts_merge_us … strong_refs`。
4. `frame_bytes_identical` 在 Windows 上会因字体指纹不同而 **SKIP**（打印指纹与本机哈希），
   这属预期；若要固定 Windows 的黄金值，请用其指纹新建一条记录。

## 7. 手动清单（≤3）

1. Windows 侧给现有的 `run_3k_*`/`run_3l_*` 脚本补 `APP_PERF_FRAMES=1`，否则将看不到 `[perf]` 行。
2. 观察 Windows 上 `bg_font_clone_us` 的量级，决定是否值得把 `load_font_list` 改为返回 `Arc`
   （属于优化，需另行确认）。
3. 若希望默认保留启动期性能输出，请说明——该行为变更的推翻条件已记入 DECISIONS。
