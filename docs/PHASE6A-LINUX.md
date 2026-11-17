# PHASE6A-LINUX：pi 在 leantty 里快捷键失效（Ctrl+D/P/C）

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论先行

**根因（已验证事实，置信度高）**：leantty 把按键交给 libghostty 的 `KeyEncoder` 时**没有提供
"未按 Shift 的码点"（unshifted codepoint）**。kitty 键盘协议对非功能键依赖该码点查表；为 0 时
上游编码器直接 `return`（不输出任何字节）。pi 启动时推送 `CSI > 7 u`（flags=1|2|4）并查询
（`CSI ? u`），leantty 如实回复 `CSI ? 7 u` 让它进入 kitty 模式，于是 **Ctrl+字母全部被静默丢弃**
（Ctrl+A/C/D/P 无任何字节），pi 收不到这些键 → 快捷键失效。

修复：编码时带上 `event.logical_key` 里的字符码点（ASCII 大写折成小写），一处调用点 + 一个纯函数。

## 1. t1 pi 的启动序列（已验证事实）

用 Python `pty` 在临时 `HOME`（`mktemp -d`）下启动 `/root/pi/pi`（v0.87.1），3 秒内抓到 2882 字节，
开头字节逐条：

| 字节 | 含义 | 需要终端应答 |
|---|---|---|
| `ESC[?2004h` | 开启括号粘贴 | 否 |
| `ESC[>7u` | **push kitty 键盘协议 flags=1\|2\|4** | 否 |
| `ESC[?u` | **查询当前 kitty flags** | **是**（终端应回 `CSI ? <flags> u`）|
| `ESC[c` | DA1（主设备属性） | 是 |
| `ESC[?25l` | 隐藏光标 | 否 |
| `OSC 11;?` | 查询背景色 | 是 |
| `ESC[?2026h` | 同步输出（BSU） | 否 |

leantty 对 `ESC[?u` 的应答（探针日志首行）：`1b5b3f3775` = `ESC[?7u` —— 与 pi 推送的 flags 一致，
故 pi 判定"终端支持 kitty 键盘协议", 后续按 CSI-u 解析按键。

pi 的终端 UI 是 `@mariozechner/pi-tui`，**已 Bun 编译进二进制**（`/root/pi` 下无 `node_modules`、
无 `dist/`），因此"源码文件与行号"无法给出——这是本阶段的一处**局限**。可从二进制字符串确认的
符号（`strings -a /root/pi/pi`）：`setKittyProtocolActive`、`isKittyProtocolActive`、
`parseKittySequence`、`matchesKittySequence`、`parseModifyOtherKeysSequence`、
`matchesModifyOtherKeys`、`isWindowsTerminal`，以及注释片段
`// Terminals with Kitty protocol flag 1 (disambiguate) send CSI-u for all keys,`
与 `* - 1 = disambiguate escape codes`。结论（工程判断，置信度高）：pi 在 kitty 激活后按
`ESC[<code>;<mods>u` 解析按键；`ctrl+字母` 期望 `ESC[<cp>;5u`。**具体回退逻辑与匹配表未核实**。

## 2. t2 探针与矩阵

探针：`scripts/e2e/keyprobe.py`（raw 模式；按 `--mode` 输出启动序列，记录终端应答与后续收到的
每个字节，附毫秒时间戳，写法 `<t_ms> <IN|OUT> <hex>`）。
运行脚本：`scripts/e2e/keymatrix.sh <mode> <log>`（在 `:99` 上起 leantty，`terminal.shell` 指向探针，
用 `xdotool key` 注入真实按键，注入前后各一个普通字母 `a` 作为"注入确实生效"的证据）。

### 2.1 修复前（已验证事实）

| 模式 | a | Enter | Esc | Tab | Up | ctrl+a | ctrl+c | ctrl+d | ctrl+p | a |
|---|---|---|---|---|---|---|---|---|---|---|
| legacy | 61 | 0d | 1b | 09 | 1b5b41 | 01 | 03 | 04 | 10 | 61 |
| kitty7 | 1b5b3f3775(应答) 61 | 0d | 1b | 09 | 1b5b313b313a3141 | **（空）** | **（空）** | **（空）** | **（空）** | 61 |

legacy 下全部正确；kitty7 下四个 Ctrl 组合键**一个字节都没有**（探针日志里连 IN 行都不出现）。

### 2.2 修复后（已验证事实）

| 模式 | ctrl+a | ctrl+c | ctrl+d | ctrl+p |
|---|---|---|---|---|
| legacy | 01 | 03 | 04 | 10 |
| kitty7 | `ESC[97;5u` | `ESC[99;5u` | `ESC[100;5u` | `ESC[112;5u` |
| mok2（modifyOtherKeys 2） | 01 | 03 | 04 | 10 |

### 2.3 参考终端对照（alacritty，已验证事实）

同探针同注入（kitty7）下 alacritty 收到：

```
61 / 0d / 1b5b323775(Esc→CSI 27u) / 09 / 1b5b41 / 1b5b39373b3575 + 1b5b39373b313a3375(按下+释放)
1b5b39393b3575 + …; 1b5b3130303b3575 …
```

即 alacritty 对 Ctrl+字母同样是 `ESC[<cp>;5u`（与 leantty 修复后一致），**并且额外发送释放事件**
（`;1:3`，因为 flags 里含 report_events）。leantty 不发释放事件——见 §6 已知限制。

## 3. t3 真实 pi 端到端（已验证事实）

临时 `HOME`、无 API key、无网络，在 leantty（`:99`）里启动真实 pi，等界面稳定后键入 `a` 再退格，
然后注入 `Ctrl-D`：**pi 进程退出**（`ps -o stat=` 为空）。修复前同一路径 pi 无反应（行为与用户报告
一致：快捷键失效）。参考终端 alacritty 上同样退出（见 §2.3 的按键级证据）。

## 4. t4 根因定位

### 4.1 三条链路梳理（已验证事实）

1. **编码器选项**：`vt.key_encoder()` 每次新建编码器并 `set_options_from_terminal(term)`
   （`crates/vt/src/terminal.rs:236-240`），因此 kitty flags / modifyOtherKeys 都从终端状态同步，
   不是陈旧的。
2. **按键数据**：`crates/app` 的 `WindowEvent::KeyboardInput` 只在 `Pressed` 时处理；有绑定先执行
   绑定，否则 `route_key(text, mapped, mods)`：**带 Ctrl/Alt 一律走 `Encoded`**，其余有 `text`
   直接写 PTY（`crates/app/src/lib.rs:156-166`）。修复前 `Encoded` 分支调用
   `enc.encode_key(key, mods, Action::Press)`——**没有码点**。
3. **查询应答路径**：`ESC[?u` 由 libghostty 的 VT 侧直接应答（`ESC[?7u`），不经过 app；app 只负责
   把子进程写的字节送进 vt。应答路径本身正常。

### 4.2 归因（候选清单对照）

| 候选 | 结论 |
|---|---|
| 应答缺失 | **排除**：探针在 1ms 内收到 `ESC[?7u` |
| 模式未同步 | **排除**：每次按键都重建编码器并同步；Up 已按 kitty 编码（`ESC[1;1:1A`）|
| 按键数据不对 | **命中**：编码事件缺 unshifted codepoint |
| 释放事件处理 | 不相关（只发 Press，pi 也不依赖释放） |

上游依据：`vendor/ghostty-src/src/input/key_encode.zig` 的 `kitty()`：非功能键用
`event.unshifted_codepoint > 0` 建表项，否则 `break :entry null`，随后 `return`（无输出）。

**推翻条件**：若在 kitty 模式下把码点填上后仍有个别键丢失（例如 Shift+组合、非 ASCII 输入法场景
或需要释放事件的程序），说明还存在第二处缺口。

## 5. t5 修复与测试

- `crates/vt/src/encoder.rs`：新增 `encode_key_full(key, mods, action, unshifted, utf8)`；
  `encode_key` 退化为 `encode_key_full(..., None, None)`（既有调用与语义不变）。
- `crates/app/src/lib.rs`：新增纯函数 `unshifted_codepoint(&logical_key)`（字符取首字符，ASCII
  大写折小写），`Encoded` 分支改调 `encode_key_full(...)` 并带上该码点。
- 单元测试（不依赖显示）：
  - `crates/vt/tests/key_kitty.rs`：模式（legacy / kitty1 / kitty7 / mok2）× 键
    （A/C/D/P + Ctrl）表驱动，断言带码点时的期望字节，并断言**不带码点时上游返回空**
    （把根因固化为回归用例）；另有功能键 Up 在两种模式下 `ESC[1;1:1A` / `ESC[A`。
  - `crates/app` 内 `unshifted_tests`：`a`/`A`/`z`/`1`/`-`/`你` 与命名键。
- e2e：`case63-key-matrix`（legacy / kitty7 / mok2 三模式 × ctrl+a/c/d/p，附两个 `a` 注入证据）、
  `case64-pi-ctrl-d`（真实 pi，临时 HOME、无 key、无网络）。
- **回退即红**：删掉 `encode_key_full` 调用、恢复 `encode_key(key, mods, Action::Press)` 后重建，
  kitty7 矩阵变成 `61 0d 1b 09 1b5b313b313a3141 (空)×4 61`（四个 Ctrl 键彻底消失）；恢复后转绿。
- 渲染未受影响：`frame_bytes_identical` 黄金哈希仍为 `507485fb7945fb8f`（字体指纹匹配，非 SKIP）。

## 6. 已知限制（本阶段新增）

- **leantty 不发送按键释放事件**（kitty flag 2 / report_events）：它会把子进程推送的 flags 如实
  回报（`ESC[?7u`），但编码时只发 Press。alacritty 同模式下会额外发 `ESC[<cp>;1:3u`。
  工程判断：多数 TUI 只用按下事件；依赖 key-up 的程序（如某些游戏/模态编辑器）会缺事件。
  推翻条件：出现某程序明确要求 `:3` 事件才能工作。
- **pi 的源码行号无法给出**：pi-tui 已编译进二进制，只有符号与注释片段可读（见 §1）。

## 7. 测试与门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | 见 §8 |
| `scripts/gates.sh` | 见 §8 |
| e2e 总数 | 见 §8 |
