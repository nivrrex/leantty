# PHASE4D-LINUX：验证"粘贴不回显 = 反色黑底黑字"假设

结论：**假设成立**。office-pc 上"粘贴后不显示、回车或再键入才出现"就是 4C 之前
反色格被画成黑底黑字所致——shell 用 readline active region（`ESC[7m … ESC[27m`）
高亮括号粘贴的文本，而该反色块在旧版本不可见。4C（`phase4c-t4`）修复反色后，
像素门禁由红转绿。

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 1. 字节流证据（已验证事实）

环境：`bash 5.2.15(1)-release`，readline bracketed paste 开启（shell 启动即发
`ESC[?2004h`）。用独立 pty 探测（不涉及本仓库代码）：

```
输入（终端 app 应发送的括号粘贴）:  ESC[200~ PASTED_MARK ESC[201~
shell 回显（实际字节）:            ESC[7m PASTED_MARK ESC[27m \r ESC[C ESC[C ESC[K
```

- `ESC[7m` = reverse video（readline 的 **active region** 高亮，bash 5.1+ 默认
  `enable-active-region on`）。
- `ESC[27m` = 关闭 reverse。
- 末尾 `\r ESC[C ESC[C ESC[K` 是 readline 重绘提示符 + 清行。

**因此粘贴文本在屏幕上是一块反色区域**：旧版本反色渲染为"原背景底 + 原背景字"
（即黑底黑字，默认调色板下完全不可见），内容却已进入 bash 的行编辑缓冲——回车即执行。
这与用户描述"内容已实际送入 shell，但当下不回显，回车或再键入才刷新"完全一致
（再键入会触发 readline 重绘、反色区域被替换为普通文本）。

## 2. 像素级门禁 case35（已验证事实，可证伪）

`scripts/e2e/run_all.sh` 的 case35（cpu 轮）与 `run_gpu_round.sh` 的同名用例：

1. 以 `[terminal.shell] program = "/bin/bash", args = ["--norc", "--noprofile"]`
   启动（确保是交互 bash、bracketed paste 默认开启）；
2. 等 bash 就绪（发出 `?2004h`）后，把 `PASTED35XX` 放入 CLIPBOARD，用**真实指针
   右键粘贴**（走 `do_paste` 完整路径，含 `normalize_paste` 的 `ESC[200~ … ESC[201~` 包裹）；
3. 截图（不是 dump）统计白色像素，阈值 **768**（与 case34 相同：10 格 × 8×16 × 0.6，
   即"反色块应占像素的 60%"，留字体/网格差异余量）。

**双版本结果**：

| 版本 | 反色块白像素 | 判定 |
|---|---|---|
| `phase4b-fix3`（4C 反色修复**前**，git worktree 检出并编译） | **4** | **红** ✅ |
| HEAD（4C 之后） | **2087** | **绿** ✅ |

cpu 后端与 gpu 后端结果一致（均 2087）。修复前 4 个白像素来自提示符字形抗锯齿边缘，
说明"文本存在但反色块不可见"，与字节流证据互相印证。

## 3. 顺带修正的钩子接线（已验证事实）

排查中发现 `APP_TEST_PASTE` 原先直接调 `paste_text`（只写 PTY，**不含
`normalize_paste` 的括号包裹**），因此门禁里 bash 收不到 `ESC[200~`，不会回显反色
——最初在 HEAD 上跑出的"白像素 4"就是被这个接线缺陷掩盖的。

已改为调 `paste_from_text`（与 `do_paste` 同一完整路径：`normalize_paste` → 写 PTY
→ 主动排帧）。因此 case32（粘贴后主动排帧）与 case35（反色可见性）验证的都是
**生产路径**。这是测试钩子的接线修正，未改变产品行为。

## 4. 改动清单

- `crates/app/src/lib.rs`：抽出 `paste_from_text()`（完整粘贴路径），`do_paste` 与
  `APP_TEST_PASTE` 钩子都调用它（钩子此前调的是不含括号包裹的 `paste_text`）。
- `scripts/e2e/run_all.sh`：新增 case35（cpu 轮）。
- `scripts/e2e/run_gpu_round.sh`：新增同名用例（gpu 轮）。
- `docs/DECISIONS.md`：假设验证结论（工程判断，置信度中高）+ 钩子接线修正。

未新增产品代码路径、未加诊断开关（假设成立，无需第 5 步）。

## 5. 验收

- `cargo test --workspace`：**129 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**91/91 PASS**，`GPU_SKIP=0`，GPU 轮 **6/6**
- Windows 目标 `cargo tree`：`wgpu`/`pollster` 计数 **0**

## 6. 未解决项

1. **真实 X11（非 Xvfb）下未验证**（待验证假设，置信度中）：本门禁在 Xvfb 上运行，
   Xvfb 有 backing store；真实 X 无 backing store 时，`present_with_damage` 的
   damage 语义与遮挡恢复行为可能不同（4B-t4 与 4B-fix1 已处理 Expose→整帧重绘，
   但未在真机确认）。若用户 4C 之后仍不回显，应优先查这里。
2. **Windows 侧未验证**：Windows 的 ConPTY + bash/WSL 或 PowerShell 是否也用反色
   高亮括号粘贴，未核实；若用 `ESC[7m` 则同样受 4C 修复影响。
3. **case35 依赖 bash 与 xclip**（已验证）：若环境无 bash 或 readline 关闭
   `enable-bracketed-paste`，用例会失效（不会误报，会因白像素不足而红）。
4. **"粘贴后主动排帧"（4B-fix3 的加固）与本次假设的关系**：本次证明用户现象由反色
   不可见导致，但该加固仍是正确的独立修复（回显延迟时主动刷新），两者不冲突。

## 7. 手动清单（≤3）

1. office-pc 用 **4C 之后的构建**（`phase4c-done` 或更新）复跑同样的粘贴操作：
   粘贴文本是否立即以反色高亮出现、回车后执行。若仍不可见 → 假设被推翻，请反馈
   （届时应查真实 X 的 present/backing store 行为）。
2. 目视确认反色块的观感（亮底暗字）在自定义 palette 下可接受。
3. Windows 上粘贴到 bash/WSL 时是否也有反色高亮（决定该结论是否跨平台）。
