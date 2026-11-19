# PHASE9C-WIN：合并 9A 收尾（GPU 字形 atlas + 启动方块）

- 日期：2026-11-19（按系统时间）
- 工作库 `E:\dev\tty`，分支 `win`；基线 `phase8c-win-done`（`86587cb`）。
- 结论分级：已验证事实 / 工程判断 / 待验证假设。

## W1 合并

| 项 | 结果 |
|---|---|
| 远端核实 | `refs/tags/phase9a-done2` = `5675ac73726cc80713d350398679eca509a9be1b`、`refs/tags/phase9a-perf` = `b785881ca90c11c8b122b8299611ace4fccf0f50`（均与任务书一致；轻量 tag） |
| 合并 1 | `b6243dd` 阶段9C W1：合并 Linux phase9a-done2（GPU 中文字形 atlas 修复），**0 冲突** |
| 合并 2 | `a4cfc41` 阶段9C W1：合并 Linux phase9a-perf（回退字体提前加载 + 省深拷贝），**0 冲突** |
| 冲突文件 | **无**（任务预期的 `docs/DECISIONS.md` 冲突未发生：两侧改动不相邻，自动合并） |

未合并 `main`，未引入 8B 观测改造。

## W2 crates 差异审计（相对 `phase8c-win-done`）

```
crates/app/src/lib.rs          | 88 +++++++++++++++++++++-------------
crates/render/src/gpu/atlas.rs | 34 ++++++++++++++++
crates/render/src/gpu/mod.rs   | 11 +++++-
crates/render/src/lib.rs       | 39 +++++++++++--------
4 files changed, 129 insertions(+), 43 deletions(-)
```

与任务预期完全一致（仅这 4 个文件，无多余文件）。内容归因：
- `atlas.rs`：新增 `GlyphAtlas::clear(queue)`（清映射与像素、整张重传、保留同一纹理）；
- `gpu/mod.rs`：`add_background_fonts` 覆盖 + `add_background_fonts_shared`；
- `render/src/lib.rs`：`load_font_list_shared` 等共享入口（旧入口委托）；
- `app/src/lib.rs`：回退字体在 `resumed` 建窗前加载、复用共享句柄。

## W3 检查（项目目录，只跑相关范围）

| 命令 | 结论 |
|---|---|
| `cargo test --workspace` | **239 passed / 0 failed**（rc=0） |
| `cargo clippy --all-targets -- -D warnings`（debug） | **rc=0、0 error** |
| `cargo clippy --release --all-targets -- -D warnings` | **rc=0、0 error** |
| `cargo check --target x86_64-pc-windows-gnu --workspace --tests` | **rc=0**（6m55s） |
| `scripts/windows/build.ps1 --release` | rc=0，`--version` = `leantty 0.2.2` |

## W4 三项运行时验证（Windows，release，**WARP/DX12 软件适配器**）

配置：`config/leantty.windows.toml` 复制；`[terminal.shell]` = `cmd.exe /c "chcp 65001 >nul & echo 中文目录 临时 & ping -n 40 127.0.0.1 >nul"`（打印一次后**静默**，避免新输出掩盖重绘）；`font.fallback = Microsoft YaHei`；`dimensions = 120×30`；GPU 跑法加 `LEANTTY_GPU_ALLOW_SOFTWARE=1`。

### A. GPU 冷启动显示真实字形，且与 CPU 逐像素比对

| 对比（960×480 客户区） | 差异像素 | 占比 | >32 通道差 | 包围盒 |
|---|---|---|---|---|
| CPU 8 s vs GPU 8 s | **516 / 460 800** | **0.1120%** | **0** | `(9,31)-(108,43)`（仅中文那一行） |
| GPU t0 vs GPU 1.6 s | 0 | 0% | 0 | — |
| CPU t0（窗口出现后 74 ms）vs CPU 1.6 s | 0 | 0% | 0 | — |

- 目视核对裁剪图：CPU 与 GPU 均显示 `中文目录 临时` 的真实字形（非方块）。
- 判定：**PASS**——GPU 冷启动即为真实字形；与 CPU 的差异仅限该行字形光栅（max ≤32），非整字缺失、非方块。

### B. `bg_fonts_ms` / `first_frame_ms`（原始 perf 行）

```
[cpu] [perf] first_frame_ms=412
[cpu] [perf] bg_fonts_ms=756 count=3
[gpu] [perf] first_frame_ms=1044
[gpu] [perf] bg_fonts_ms=1005 count=3
（另一次 gpu 运行：first_frame_ms=461 / bg_fonts_ms=647）
```

- 本机**未观察到方块期**：窗口出现后的首张截图（CPU 74 ms、GPU 118 ms）已是中文。
- 与 Linux 记录的差异如实保留，不改阈值；Windows 方块期实测 **0 ms（未捕获到）**，用户机若 >0.5 s 属已知遗留（单面 CJK 解析限制），本任务不优化。

### C. 老的“3 秒才自动恢复”症状

- GPU 2 s 与 8 s 截图**完全相同（0 像素差）**；GPU 与 CPU 仅在字形光栅上差 516 px（0.112%，≤32）→ 无“先方块、3 s 后恢复”的迹象。
- 判定：**PASS**（等价于 `case81` 的“CPU/GPU 就绪后像素一致”逻辑）。

## W5 打包

```
zip:    vt-verify-win/leantty-0.2.2-windows-x86_64.zip
size:   5 181 473 bytes
sha256: 29944c2a3a4227b055c4d76648d04cfea50de075da8e881cdf71d80d5f71a96f
entries (6): leantty.exe, config.toml, README.md, README.zh-CN.md, LICENSE, THIRD-PARTY-NOTICES.md
exe:    12f4b46823b1b7193015d30dfbb9b406a4b8773510150955af911c956d5fc9b6
```

- 条目与任务书一致（6 项，`config.toml` 来自 `config/leantty.windows.toml`，`sha256 8824111b…`）。
- **与上一版 zip 的差异（如实记录）**：0.2.2 上一版为 10 条目（含 `conpty.dll`、`OpenConsole.exe`、`leantty.toml` 等）；
  本版按任务书只含 6 项 → **不再随包携带 ConPTY 对**，运行时将回落到系统 conhost（会打印既有的成对部署警告）。
  如果需要随附 ConPTY，请用 `scripts/windows/package-release.ps1` 生成 10 条目版本。

## 未验证项 / 已知限制

1. 仅 WARP/DX12 **软件**适配器可用，未在真实 GPU 上验证 A/B/C。
2. Windows 冷启动“方块期”未捕获到（首帧即中文）；用户机若仍有 >0.5 s 方块，属 Linux 已裁定的遗留问题，本次未优化。
3. 本版 zip 不含 ConPTY 对（见 W5 说明）。
4. 鼠标/键盘注入不可用：`case81` 的交互形态未在 Windows 复现（用截图 + perf 替代）。
