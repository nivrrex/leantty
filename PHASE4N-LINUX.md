# PHASE4N-LINUX：合并 Windows 3K / 随附 ConPTY 决定 / 字体内存取证与共享化

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3k-win-done` | 已合并 `b42f025`（tag `phase4n-t1`），共用代码零改动 |
| t1b README 随附 ConPTY | 新增一节 + 平台限制条目加限定语，DECISIONS 记录决定 |
| t2 字体内存取证 | 单实例 298.7MB、clone 100% 深拷贝、"每个回退面 4 份"实测成立（tag `phase4n-t2`） |
| t3 共享化 | 改 `Arc<Font>`：CJK 回退增量 1508MB→311MB，逐字节不变（tag `phase4n-t3`） |

## 1. t1 合并证据（ls-remote 实测）

```
2d08837d5027c8be57e6edc20c68eae49f9178dd	refs/heads/main（合并前）
e1723994caa35fdac308ba07d7991b3dd8c68c07	refs/heads/win
8dac33272255579d3a9f759104b596d83e5bff7a	refs/tags/phase3k-win-done
b42f02580cdcce7b99e34b69ff02c15317cdb992	refs/tags/phase3k-win-done^{}
```

- tag 是附注标签，解引用 `^{}` = **b42f025**；`refs/heads/win` = **e172399**，比它多一个
  "补记 hash" 提交（仅改 `PHASE3K-WIN.md`，+7/−1）。按任务要求合并 **b42f025**。
- 该合并同时带上一轮（4M）漏下的 `5ffd6fa`（3J 的补记 hash 提交）——因为 4M 合并的是
  `eb2b8ae`，而 3K 分支以 `5ffd6fa` 为祖先。
- 共用/受控范围：`.gitignore` 一行（`/vt-verify-win/conpty-dl/`）、`PHASE3J-WIN.md`、
  `PHASE3K-WIN.md`、`docs/WIN-NOTES.md`；**`crates/` 零差异**；新增 `scripts/windows/conpty/`
  （`LICENSE-ConPTY.txt`、`NOTICE.txt`、`README.md`、`fetch-conpty.ps1`）。
- 合并后 `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 通过、依赖树
  `wgpu`/`pollster` 命中 0。

## 2. t1b README 增量

新增 **「Windows：随附官方 ConPTY」** 一节，只引用 `PHASE3K-WIN.md` 已实测内容：

| 条目 | 内容 | 依据 |
|---|---|---|
| 包与版本 | NuGet `Microsoft.Windows.Console.ConPTY` **1.25.261109001**（MIT，最新稳定） | §1.2 |
| 部署 | `conpty.dll` 与 `OpenConsole.exe` 在包内不同目录，取出后**必须与 `app.exe` 同目录** | §1.3 / §2 |
| 获取与校验 | `scripts/windows/conpty/fetch-conpty.ps1`（`-OutDir`/`-Proxy`）：包 sha256 + 两文件 sha256 + Authenticode 必须 Valid 且主体 Microsoft | §1.3 / §2 |
| 回退 | **删除或改名这两个文件**即回到系统 conhost | §2 |
| 对照 | 折行不再物化 CRLF（S: `80/80/43` 分段；N: 单段 200 字符）；`ESC[7m` 原样到达（S 改写为 `ESC[30m`+`ESC[47m`）；`OSC 104` 可达（S/W 丢弃） | §3.2 / §3.3 |
| 代价 | `first_frame` 中位 1673→1787ms（**+约 110ms**）；2M 行 330→355s（**约慢 7%**）；关闭/粘贴/写线程停顿无劣化 | §3.6 / §3.7 / §3.5 / §3.8 / §3.9 |
| 门槛 | §3.10（a/d/e/h/i 无新增失败 且 b 或 c 至少一项优于 S） | §3.10 |

「已知平台限制」中受影响的四条（`ESC[7m` 物化、`OSC 104` 不到达、折行物化 CRLF）各加注
**"使用系统 conhost 时仍适用"** 并指向 3K 的改善证据；GPU 仅 Linux 一条不受影响。

## 3. t2 字体内存取证

### 3.1 单实例大小与 clone 成本（`crates/render/tests/font_memory.rs`，std-only 计数分配器）

| 字体面 | 文件字节 | 实例 MB | clone 增量 MB | clone/实例 |
|---|---|---|---|---|
| DejaVuSansMono.ttf | 343140 | **8.9** | 8.9 | 100% |
| NotoSansCJK-Regular.ttc（SC） | 19484784 | **298.7** | 298.7 | 100% |
| NotoSansCJK-Regular.ttc（JP，同一文件另一面） | 19484784 | **298.7** | 298.7 | 100% |

释放后回落到基线 → 计数可信。

### 3.2 实例数（静态推演，与实测吻合）

- `build_font_groups`：四个样式组，每组主字体一个实例；同一 face 由 `face_cache` 去重
  （但缓存里存的是 `Font` 的**深拷贝**，命中时再 clone 返回 → 每个 face 至少 2 份瞬时）。
- `load_font_list`：回退字体 + **2 个系统兜底字体**（Monospace/SansSerif）。
- `CpuRenderer::add_background_fonts`：`for g in 4 组 { g.extend(fonts.iter().cloned()) }`
  → **每个回退字体面 4 份**（外加 1 份瞬时原值）。

### 3.3 RSS 时间线（`scripts/e2e/font_mem_probe.sh`，release/cpu，空闲）

| 配置 | 结束 RSS | 相对无回退 | 折算份数 |
|---|---|---|---|
| 无回退 | 209376KB | — | — |
| `黑体`（fontconfig → simhei.ttf 9753388B） | 862332KB | +652956KB ≈ 637MB | ≈4×159MB |
| `Noto Sans CJK JP`（同一 19MB .ttc 另一面） | 1754480KB | +1545104KB ≈ 1508MB | ≈4×300MB（+余量） |

采样：启动后每 50ms 至 3s、之后每 5s；`bg_fonts_ms` 约 383–1226ms（与 RSS 台阶同步）。
gpu 后端同量级（+633MB / +1504MB），基线另高约 181MB 属 GPU 栈。

### 3.4 与 Windows 690MB 差的对照（工程判断，置信度高）

Windows 空闲 879MB − 无回退 189MB = 690MB；按 4 份折算 ≈ **172MB/实例**，与大型 CJK 字体
（YaHei 级）经 fontdue 解析后的量级自洽。**假设成立**：倍增来自"同一字体面 4 份 + clone 深拷贝"，
而非解析密度问题。（YaHei 的具体实例大小未在 Linux 取得 → 标"未核实"。）

## 4. t3 实施：`Arc<Font>` 共享

- `build_font_groups` → `Vec<Vec<Arc<Font>>>`；`face_cache` → `HashMap<fontdb::ID, Arc<Font>>`，
  命中返回 `Arc::clone`（不再深拷贝）；`add_background_fonts` 每个回退字体只 `Arc::new` 一次，
  四个样式组共享；CPU 与 GPU 渲染器同一类型；`font_groups()`/`new_surface_with_groups()`
  的公共类型随之变化。**未改 trait、未换库、未加依赖。**

### 4.1 效果

| 项 | 改前 | 改后 |
|---|---|---|
| RSS 无回退 | 209376KB | **88432KB** |
| `黑体` 增量 | +652956KB（637MB） | **+132444KB（129MB ≈ 1 份）** |
| `Noto Sans CJK JP` 增量 | +1545104KB（1508MB） | **+318888KB（311MB ≈ 1 份 298.7MB）** |
| 唯一实例数（4 主 + 1 回退） | 4 + 4 = 8 | **4 + 1 = 5** |
| `first_frame` 中位（含 CJK 回退，各 5 次） | 208ms（189/201/208/210/229） | **184ms（176/180/184/191/192）** |
| 6 分钟 soak RSS 增长 | — | CPU **+3.2%**、GPU **+0.3%**，无 panic |

> 无回退基线也从 209MB 降到 88MB：因为 2 个系统兜底字体（DejaVu ≈8.9MB）此前同样被复制 4 份。

### 4.2 逐字节不变（黄金校验）

新增 `crates/render/tests/frame_bytes_identical.rs`：固定 VT 序列（ASCII / 粗体 / 反色 /
下划线 / 宽字符 / CJK / 光标定位）+ 固定调色板，渲染 40×12 格 → 帧缓冲 FNV-1a64
= **`507485fb7945fb8f`**，**改前与改后完全相同**，已固化为 `GOLDEN`。

> 说明：`APP_FRAME_HASH`（debug 钩子）的**跨运行**对拍不可用——同一二进制连跑两次哈希即不同
> （实测），因为它把交互时序/帧数也带进比较。故改用上述确定性测试。

既有像素类用例全部保持通过：`gpu_parity`（1 LSB 门禁）、`glyph_clip`、`baseline_alignment`、
`wide_glyph`、`inverse_pixels`、`inverse_default_color`、`palette_config_render`、`cursor_viewport_pixels`；
e2e 105/105（含 case26 增量 vs 全量 AE=0、case34/35/36 像素计数）。

### 4.3 回归保护与证伪

`crates/render/tests/font_sharing.rs`：用 `Arc::as_ptr` 去重计数，断言
①四个样式组是 4 个唯一实例；②组内无重复实例；③加入 1 个回退字体后唯一实例数**只 +1**；
④四组尾部指向同一指针。把 `add_background_fonts` 改回"按组 `Arc::new(f.clone())`"时该测试
**红**（实测 4→8），恢复即绿。

## 5. 未解决问题

1. **`face_cache` 命中仍返回 `Arc::clone` 意味着主字体四个样式若指向同一 face 会共享**——
   本轮四个样式是不同 face，故未验证该路径的实际收益（工程判断，置信度中）。
2. **GPU 后端基线另高约 181MB**（wgpu/lavapipe 栈），与字体无关，未拆分。
3. **Windows 的 YaHei 单实例大小未在本机取得**（未核实）：690MB 的折算只到量级自洽。

## 6. Windows 交接

1. 随附 ConPTY 的决定已写入 README 与 DECISIONS；Windows 侧只需 `fetch-conpty.ps1` 取一次并放同目录。
2. `Arc<Font>` 改动与平台无关：Windows 侧应看到空闲内存大幅下降（690MB 差应降到约 1 份
   YaHei 实例）。请复测 3K 的 `run_3k_mem.ps1` 以确认。
3. 若 Windows 上仍有"每面多份"，先看是否走了另一条平台分支（ConPTY 与系统 conhost 无关）。

## 7. 手动清单（≤3）

1. Windows 上按 README 取一次 ConPTY 并放同目录，确认 `OpenConsole.exe` 为 1.25.2611.09001。
2. Windows 复测空闲内存，确认 879MB → 约"主字体 4 份 + 回退 1 份"的量级。
3. 真实使用中观察首帧是否仍为 184ms 量级（本轮 Linux 上略优于改前）。
