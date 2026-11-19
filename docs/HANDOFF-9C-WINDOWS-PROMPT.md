# 给 Windows Agent 的提示词（9C 返工 / 收尾，直接复制粘贴）

> 背景：9C 的合并与验证已基本完成（`win` 分支 HEAD `c9827b3`，未推送、未合 main、未打 tag）。
> 下面三件事做完再推分支；**不要打 tag**（`scripts/tag_checked.sh` 要求 `gates_ok` 与 HEAD 一致，只能在 Linux 侧产生）。

---

## 提示词正文

你要在 Windows 机器上完成 leantty 9C 的收尾返工。**不要改任何源码、阈值、判据**；所有问题都在打包与取证层面。

### 任务 1：按已发布的 0.2.2 结构重打 zip（必须）

上一版交付物是 6 条目、且把配置写成 `config.toml`，与已发布版本不一致，**缺 ConPTY 对会直接回落系统 conhost，属于行为回退**。要求：

1. 用 `scripts/windows/package-release.ps1` 重打，产物必须与已发布的 0.2.2 同结构 **10 条目**，路径前缀为 `leantty-0.2.2-windows-x86_64/`：
   ```
   CHANGELOG.md
   conpty.dll
   leantty.exe
   leantty.toml              ← 不是 config.toml
   leantty-portable.cmd
   LICENSE
   OpenConsole.exe
   README.md
   README.zh-CN.md
   THIRD-PARTY-NOTICES.md
   ```
2. 与参考版本逐项对照（参考：已发布 `leantty-0.2.2-windows-x86_64.zip`，5,793,973 B，sha256 `f80d82fe6f5e26da7eb4f8e746a957fdbf6e7e6fbb71b771fa2355001ce70538`，10 条目）：
   - **必须逐字节一致**：`conpty.dll`（119,608 B）、`OpenConsole.exe`（1,089,336 B）、`LICENSE`（1,077 B）、`THIRD-PARTY-NOTICES.md`（149,116 B）、`leantty-portable.cmd`（74 B）、`leantty.toml`（3,449 B）。
   - **允许不同**（因为本版含 9A 文档改动）：`leantty.exe`、`CHANGELOG.md`、`README.md`、`README.zh-CN.md`。
3. 打包后贴 `unzip -l` 的完整输出（条目名 + 大小），并说明每个"允许不同"的项比参考大了/小了多少。
4. 体积预期 ≈ 5.8–5.9 MB（参考 5,793,973 B）；若明显偏小，先自查是否又漏了 ConPTY 对。

### 任务 2：复测 CPU 启动方块期（必须，用于消除矛盾）

你的报告里 CPU 的 `first_frame_ms=412`、`bg_fonts_ms=756`（= 应有约 344 ms 方块期），但同报告又说"窗口出现后 74 ms 的截图已是中文"（绝对约 486 ms < 756 ms）。两者不能同时成立。要求：

1. 以**进程启动为 t=0**，在 t=0.15 / 0.40 / 0.80 / 1.20 s 各截一张图（`LEANTTY_PERF_FRAMES=1`，同一进程内完成，不要跨运行拼数据）。
2. 同一次运行里记录 `first_frame_ms`、`bg_fonts_ms`、`font.fallback` 实际生效的字体族名。
3. 逐张说明"方块 / 中文"，并给出结论：
   - 若 t=0.40 s 已是中文：说明原因（中文由哪条字体链渲染、或那次 fallback 是否真的生效）。
   - 若确实存在方块期：**照实记录数值**，这是 Linux 侧已裁定的遗留问题（README 已知限制、DECISIONS 有据），**不得为此改代码或阈值**。
4. 不需要重测 GPU（`first_frame 1044 / bg_fonts 1005` 自洽，即字体先到、首帧即中文）。

### 任务 3：给出 DECISIONS 的合并证据（必须）

合并报告称"0 冲突"，需要证据：

1. `git log --oneline -- docs/DECISIONS.md`
2. `grep -n "0.31 s\|遗留问题保留\|phase9a\|启动早期" docs/DECISIONS.md`
3. `tail -n 12 docs/DECISIONS.md`

确认 Linux 侧本次新增的两条都在（①启动早期方块作为遗留问题保留，附 230/545/305 ms 实测；②9A GPU 字形 atlas 修复），且 Windows 侧原有条目一条未丢。

### 任务 4：推送

- 三项都完成后 `git push origin win`（**只推 win，不动 main**）。
- **不打 tag**；若你认为需要 tag，写在交付里交给 Linux 侧裁决。
- 不要 force push、不要 rebase 已推送的历史。

### 交付格式（照这个回）
```
合并后 HEAD: <sha>（+ 是否已 push）
zip: leantty-0.2.2-windows-x86_64.zip  <字节数>  <sha256>
zip 条目: <10 条目名与大小；或 unzip -l 输出>
逐项对照: conpty.dll/OpenConsole.exe/LICENSE/THIRD-PARTY/leantty-portable.cmd/leantty.toml 是否字节一致；exe/CHANGELOG/README 差异量
exe: <sha256>
CPU 时间线: t=0.15 <方块|中文> / t=0.40 <...> / t=0.80 <...> / t=1.20 <...>；first_frame_ms=<v> bg_fonts_ms=<v> fallback=<字体名>
CPU 结论: <有方块期=?ms；或为何无>
DECISIONS 证据: <三条命令的输出摘要>
未验证项: <列表>
```
