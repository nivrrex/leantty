# PHASE6B-LINUX：全键位对拍与按键修复

## 1. 基线（t1f）

leantty 与 alacritty 在 legacy / kitty flags=7 两种模式下各测 98 键；每键一次干净会话，注入 `a`(61) 作为哨兵切段。

驱动：`scripts/e2e/keydiff.sh`，键表：`scripts/e2e/keylist.txt`，探针：`scripts/e2e/keyprobe.py`。

## 2. 矩阵（修复前）

| 键 | leantty legacy | alacritty legacy | leantty kitty7 | alacritty kitty7 |
|---|---|---|---|---|
| `ctrl+a` | `01` | `01` | `1b5b39373b3575` | `1b5b39373b313a3375 1b5b39373b3575 1b5b39373b313a3375` |
| `ctrl+b` | `02` | `02` | `1b5b39383b3575` | `1b5b39373b313a3375 1b5b39383b3575 1b5b39383b313a3375` |
| `ctrl+c` | `03` | `03` | `1b5b39393b3575` | `1b5b39373b313a3375 1b5b39393b3575 1b5b39393b313a3375` |
| `ctrl+d` | `04` | `04` | `1b5b3130303b3575` | `1b5b39373b313a3375 1b5b3130303b3575 1b5b3130303b313a3375` |
| `ctrl+e` | `05` | `05` | `1b5b3130313b3575` | `1b5b39373b313a3375 1b5b3130313b3575 1b5b3130313b313a3375` |
| `ctrl+f` | `06` | `06` | `1b5b3130323b3575` | `1b5b39373b313a3375 1b5b3130323b3575 1b5b3130323b313a3375` |
| `ctrl+g` | `07` | `07` | `1b5b3130333b3575` | `1b5b39373b313a3375 1b5b3130333b3575 1b5b3130333b313a3375` |
| `ctrl+h` | `08` | `08` | `1b5b3130343b3575` | `1b5b39373b313a3375 1b5b3130343b3575 1b5b3130343b313a3375` |
| `ctrl+i` | `` | `09` | `1b5b3130353b3575` | `1b5b39373b313a3375 1b5b3130353b3575 1b5b3130353b313a3375` |
| `ctrl+j` | `0a` | `0a` | `1b5b3130363b3575` | `1b5b39373b313a3375 1b5b3130363b3575 1b5b3130363b313a3375` |
| `ctrl+k` | `0b` | `0b` | `1b5b3130373b3575` | `1b5b39373b313a3375 1b5b3130373b3575 1b5b3130373b313a3375` |
| `ctrl+l` | `0c` | `0c` | `1b5b3130383b3575` | `1b5b39373b313a3375 1b5b3130383b3575 1b5b3130383b313a3375` |
| `ctrl+m` | `` | `0d` | `1b5b3130393b3575` | `1b5b39373b313a3375 1b5b3130393b3575 1b5b3130393b313a3375` |
| `ctrl+n` | `0e` | `0e` | `1b5b3131303b3575` | `1b5b39373b313a3375 1b5b3131303b3575 1b5b3131303b313a3375` |
| `ctrl+o` | `0f` | `0f` | `1b5b3131313b3575` | `1b5b39373b313a3375 1b5b3131313b3575 1b5b3131313b313a3375` |
| `ctrl+p` | `10` | `10` | `1b5b3131323b3575` | `1b5b39373b313a3375 1b5b3131323b3575 1b5b3131323b313a3375` |
| `ctrl+q` | `11` | `11` | `1b5b3131333b3575` | `1b5b39373b313a3375 1b5b3131333b3575 1b5b3131333b313a3375` |
| `ctrl+r` | `12` | `12` | `1b5b3131343b3575` | `1b5b39373b313a3375 1b5b3131343b3575 1b5b3131343b313a3375` |
| `ctrl+s` | `13` | `13` | `1b5b3131353b3575` | `1b5b39373b313a3375 1b5b3131353b3575 1b5b3131353b313a3375` |
| `ctrl+t` | `14` | `14` | `1b5b3131363b3575` | `1b5b39373b313a3375 1b5b3131363b3575 1b5b3131363b313a3375` |
| `ctrl+u` | `15` | `15` | `1b5b3131373b3575` | `1b5b39373b313a3375 1b5b3131373b3575 1b5b3131373b313a3375` |
| `ctrl+v` | `16` | `16` | `1b5b3131383b3575` | `1b5b39373b313a3375 1b5b3131383b3575 1b5b3131383b313a3375` |
| `ctrl+w` | `17` | `17` | `1b5b3131393b3575` | `1b5b39373b313a3375 1b5b3131393b3575 1b5b3131393b313a3375` |
| `ctrl+x` | `18` | `18` | `1b5b3132303b3575` | `1b5b39373b313a3375 1b5b3132303b3575 1b5b3132303b313a3375` |
| `ctrl+y` | `19` | `19` | `1b5b3132313b3575` | `1b5b39373b313a3375 1b5b3132313b3575 1b5b3132313b313a3375` |
| `ctrl+z` | `1a` | `1a` | `1b5b3132323b3575` | `1b5b39373b313a3375 1b5b3132323b3575 1b5b3132323b313a3375` |
| `alt+a` | `1b61` | `1b61` | `1b5b39373b3375` | `1b5b39373b313a3375 1b5b39373b3375 1b5b39373b313a3375` |
| `alt+b` | `1b62` | `1b62` | `1b5b39383b3375` | `1b5b39373b313a3375 1b5b39383b3375 1b5b39383b313a3375` |
| `alt+f` | `1b66` | `1b66` | `1b5b3130323b3375` | `1b5b39373b313a3375 1b5b3130323b3375 1b5b3130323b313a3375` |
| `alt+d` | `1b64` | `1b64` | `1b5b3130303b3375` | `1b5b39373b313a3375 1b5b3130303b3375 1b5b3130303b313a3375` |
| `alt+BackSpace` | `1b7f` | `1b7f` | `1b5b3132373b3375` | `1b5b39373b313a3375 1b5b3132373b3375` |
| `alt+Return` | `1b0d` | `1b0d` | `1b5b31333b3375` | `1b5b39373b313a3375 1b5b31333b3375` |
| `Return` | `0d` | `0d` | `0d` | `1b5b39373b313a3375 0d` |
| `shift+Return` | `0d` | `0d` | `0d` | `1b5b39373b313a3375 1b5b31333b3275` |
| `ctrl+Return` | `1b5b32373b353b31337e` | `0d` | `1b5b31333b3575` | `1b5b39373b313a3375 1b5b31333b3575` |
| `Tab` | `09` | `09` | `09` | `1b5b39373b313a3375 09` |
| `shift+Tab` | `1b5b5a` | `1b5b5a` | `1b5b393b3275` | `1b5b39373b313a3375 1b5b393b3275` |
| `Escape` | `1b` | `1b` | `1b` | `1b5b39373b313a3375 1b5b323775 1b5b32373b313a3375` |
| `BackSpace` | `08` | `7f` | `08` | `1b5b39373b313a3375 7f` |
| `Delete` | `7f` | `1b5b337e` | `7f` | `1b5b39373b313a3375 1b5b337e 1b5b333b313a337e` |
| `Insert` | `` | `1b5b327e` | `` | `1b5b39373b313a3375 1b5b327e 1b5b323b313a337e` |
| `Home` | `1b5b48` | `1b5b48` | `1b5b313b313a3148` | `1b5b39373b313a3375 1b5b48 1b5b313b313a3348` |
| `End` | `1b5b46` | `1b5b46` | `1b5b313b313a3146` | `1b5b39373b313a3375 1b5b46 1b5b313b313a3346` |
| `Prior` | `` | `1b5b357e` | `` | `1b5b39373b313a3375 1b5b357e 1b5b353b313a337e` |
| `Next` | `` | `1b5b367e` | `` | `1b5b39373b313a3375 1b5b367e 1b5b363b313a337e` |
| `Up` | `1b5b41` | `1b5b41` | `1b5b313b313a3141` | `1b5b39373b313a3375 1b5b41 1b5b313b313a3341` |
| `Down` | `1b5b42` | `1b5b42` | `1b5b313b313a3142` | `1b5b39373b313a3375 1b5b42 1b5b313b313a3342` |
| `Left` | `1b5b44` | `1b5b44` | `1b5b313b313a3144` | `1b5b39373b313a3375 1b5b44 1b5b313b313a3344` |
| `Right` | `1b5b43` | `1b5b43` | `1b5b313b313a3143` | `1b5b39373b313a3375 1b5b43 1b5b313b313a3343` |
| `shift+Up` | `1b5b313b3241` | `1b5b313b3241` | `1b5b313b323a3141` | `1b5b39373b313a3375 1b5b313b3241 1b5b313b313a3341` |
| `shift+Down` | `1b5b313b3242` | `1b5b313b3242` | `1b5b313b323a3142` | `1b5b39373b313a3375 1b5b313b3242 1b5b313b313a3342` |
| `shift+Left` | `1b5b313b3244` | `1b5b313b3244` | `1b5b313b323a3144` | `1b5b39373b313a3375 1b5b313b3244 1b5b313b313a3344` |
| `shift+Right` | `1b5b313b3243` | `1b5b313b3243` | `1b5b313b323a3143` | `1b5b39373b313a3375 1b5b313b3243 1b5b313b313a3343` |
| `alt+Up` | `1b5b313b3341` | `1b5b313b3341` | `1b5b313b333a3141` | `1b5b39373b313a3375 1b5b313b3341 1b5b313b313a3341` |
| `alt+Down` | `1b5b313b3342` | `1b5b313b3342` | `1b5b313b333a3142` | `1b5b39373b313a3375 1b5b313b3342 1b5b313b313a3342` |
| `alt+Left` | `1b5b313b3344` | `1b5b313b3344` | `1b5b313b333a3144` | `1b5b39373b313a3375 1b5b313b3344 1b5b313b313a3344` |
| `alt+Right` | `1b5b313b3343` | `1b5b313b3343` | `1b5b313b333a3143` | `1b5b39373b313a3375 1b5b313b3343 1b5b313b313a3343` |
| `ctrl+Up` | `1b5b313b3541` | `1b5b313b3541` | `1b5b313b353a3141` | `1b5b39373b313a3375 1b5b313b3541 1b5b313b313a3341` |
| `ctrl+Down` | `1b5b313b3542` | `1b5b313b3542` | `1b5b313b353a3142` | `1b5b39373b313a3375 1b5b313b3542 1b5b313b313a3342` |
| `ctrl+Left` | `1b5b313b3544` | `1b5b313b3544` | `1b5b313b353a3144` | `1b5b39373b313a3375 1b5b313b3544 1b5b313b313a3344` |
| `ctrl+Right` | `1b5b313b3543` | `1b5b313b3543` | `1b5b313b353a3143` | `1b5b39373b313a3375 1b5b313b3543 1b5b313b313a3343` |
| `F1` | `` | `1b4f50` | `` | `1b5b39373b313a3375 1b5b50 1b5b313b313a3350` |
| `F2` | `` | `1b4f51` | `` | `1b5b39373b313a3375 1b5b51 1b5b313b313a3351` |
| `F3` | `` | `1b4f52` | `` | `1b5b39373b313a3375 1b5b31337e 1b5b31333b313a337e` |
| `F4` | `` | `1b4f53` | `` | `1b5b39373b313a3375 1b5b53 1b5b313b313a3353` |
| `F5` | `` | `1b5b31357e` | `` | `1b5b39373b313a3375 1b5b31357e 1b5b31353b313a337e` |
| `F6` | `` | `1b5b31377e` | `` | `1b5b39373b313a3375 1b5b31377e 1b5b31373b313a337e` |
| `F7` | `` | `1b5b31387e` | `` | `1b5b39373b313a3375 1b5b31387e 1b5b31383b313a337e` |
| `F8` | `` | `1b5b31397e` | `` | `1b5b39373b313a3375 1b5b31397e 1b5b31393b313a337e` |
| `F9` | `` | `1b5b32307e` | `` | `1b5b39373b313a3375 1b5b32307e 1b5b32303b313a337e` |
| `F10` | `` | `1b5b32317e` | `` | `1b5b39373b313a3375 1b5b32317e 1b5b32313b313a337e` |
| `F11` | `` | `1b5b32337e` | `` | `1b5b39373b313a3375 1b5b32337e 1b5b32333b313a337e` |
| `F12` | `` | `1b5b32347e` | `` | `1b5b39373b313a3375 1b5b32347e 1b5b32343b313a337e` |
| `space` | `20` | `20` | `20` | `1b5b39373b313a3375 20 1b5b33323b313a3375` |
| `ctrl+space` | `` | `00` | `` | `1b5b39373b313a3375 1b5b33323b3575 1b5b33323b313a3375` |
| `ctrl+bracketleft` | `` | `1b` | `` | `1b5b39373b313a3375 1b5b39313b3575 1b5b39313b313a3375` |
| `ctrl+bracketright` | `` | `1d` | `` | `1b5b39373b313a3375 1b5b39333b3575 1b5b39333b313a3375` |
| `ctrl+backslash` | `` | `1c` | `` | `1b5b39373b313a3375 1b5b39323b3575 1b5b39323b313a3375` |
| `ctrl+slash` | `` | `1f` | `` | `1b5b39373b313a3375 1b5b34373b3575 1b5b34373b313a3375` |
| `ctrl+minus` | `` | `` | `` | `1b5b39373b313a3375 1b5b34353b313a3375` |
| `ctrl+equal` | `` | `` | `` | `1b5b39373b313a3375 1b5b36313b313a3375` |
| `ctrl+comma` | `` | `2c` | `` | `1b5b39373b313a3375 1b5b34343b3575 1b5b34343b313a3375` |
| `ctrl+period` | `` | `2e` | `` | `1b5b39373b313a3375 1b5b34363b3575 1b5b34363b313a3375` |
| `ctrl+semicolon` | `` | `3b` | `` | `1b5b39373b313a3375 1b5b35393b3575 1b5b35393b313a3375` |
| `ctrl+apostrophe` | `` | `27` | `` | `1b5b39373b313a3375 1b5b33393b3575 1b5b33393b313a3375` |
| `ctrl+1` | `31` | `31` | `1b5b34393b3575` | `1b5b39373b313a3375 1b5b34393b3575 1b5b34393b313a3375` |
| `ctrl+2` | `00` | `00` | `1b5b35303b3575` | `1b5b39373b313a3375 1b5b35303b3575 1b5b35303b313a3375` |
| `ctrl+3` | `1b` | `1b` | `1b5b35313b3575` | `1b5b39373b313a3375 1b5b35313b3575 1b5b35313b313a3375` |
| `ctrl+4` | `1c` | `1c` | `1b5b35323b3575` | `1b5b39373b313a3375 1b5b35323b3575 1b5b35323b313a3375` |
| `ctrl+5` | `1d` | `1d` | `1b5b35333b3575` | `1b5b39373b313a3375 1b5b35333b3575 1b5b35333b313a3375` |
| `ctrl+6` | `1e` | `1e` | `1b5b35343b3575` | `1b5b39373b313a3375 1b5b35343b3575 1b5b35343b313a3375` |
| `ctrl+7` | `1f` | `1f` | `1b5b35353b3575` | `1b5b39373b313a3375 1b5b35353b3575 1b5b35353b313a3375` |
| `ctrl+8` | `7f` | `7f` | `1b5b35363b3575` | `1b5b39373b313a3375 1b5b35363b3575 1b5b35363b313a3375` |
| `ctrl+9` | `39` | `39` | `1b5b35373b3575` | `1b5b39373b313a3375 1b5b35373b3575 1b5b35373b313a3375` |
| `shift+1` | `21` | `21` | `21` | `1b5b39373b313a3375 21 1b5b34393b313a3375` |
| `shift+semicolon` | `3a` | `3a` | `3a` | `1b5b39373b313a3375 3a 1b5b35393b313a3375` |
| `shift+slash` | `3f` | `3f` | `3f` | `1b5b39373b313a3375 3f 1b5b34373b313a3375` |
| `ctrl+shift+semicolon` | `` | `3a` | `` | `1b5b39373b313a3375 1b5b35393a35383b3675 1b5b35393b313a3375` |

## 3. t2 诊断与修复（t2i / t2j）

### 3.1 上游编码器行为（已验证事实，`vendor/ghostty-src/src/input/key_encode.zig`）

- `kitty()`（第 100 行起）：非功能键靠 `event.unshifted_codepoint > 0` 建表项（第 139-151 行），
  为 0 时 `break :entry null` 后 `return`（无输出）→ **应用必须提供码点**。
- `legacy()` 里控制字符由 `ctrlSeq()`（第 708 行起）负责：`char` 取 `utf8[0]`（仅当 `utf8.len == 1`）
  否则取 `logical_key.codepoint()`；随后 `@`–`_` 折成 C0（`[`→0x1b、`\`→0x1c、`]`→0x1d、
  `^`→0x1e、`_`→0x1f、`@`→0x00）；**其它标点（`;` `,` `.` `'`）不在该范围内，直接返回 null**
  （未核实是否属上游有意为之）。

### 3.2 探测矩阵（`crates/vt` 单测，穷举 utf8 × unshifted）

| 键 | legacy（带码点） | kitty7（带码点） |
|---|---|---|
| Space | `00` | `ESC[32;5u` |
| BracketLeft | **空** | `ESC[91;5u` |
| BracketRight | `1d` | `ESC[93;5u` |
| Backslash | `1c` | `ESC[92;5u` |
| Slash | `1f` | `ESC[47;5u` |
| Semicolon / Comma / Period / Quote | **空** | `ESC[59;5u` / `44` / `46` / `39` |
| I / M | **空** | `ESC[105;5u` / `ESC[109;5u` |

**结论分类**：
1. **类别 1（应用数据有误）**：kitty 下全部符号键/字母键——补上码点后编码器即输出期望字节（已修）。
2. **类别 2（编码器无法输出）**：legacy 的 `ctrl+[`（已按裁决回写 `0x1b`，仅此一键）；`ctrl+space`
   在 legacy 其实**能**输出 `00`（见上表），端到端测到空输出，疑似注入侧（xdotool/WM）吞键，标**待核实**。
3. **类别 3（参考终端特有 / 上游行为）**：legacy 的 `ctrl+;` `ctrl+,` `ctrl+.` `ctrl+'`
   `ctrl+shift+;` `ctrl+i` `ctrl+m` —— alacritty 回写原始字符或 `09`/`0d`，编码器不提供；按裁决不修补。

### 3.3 修复内容

- `map_logical_key`：补 `Insert / PageUp / PageDown / Delete / F1–F12` 与符号键
  （Minus Equal Comma Period Semicolon Quote Backquote Backslash BracketLeft BracketRight Slash Space）。
- `route_key`：整串为控制字符的 text（如 Delete 的 `0x7f`）改走编码器。
- `unshifted_codepoint`：优先 `KeyEventExtModifierSupplement::key_without_modifiers`
  （winit-0.30.13 `src/platform/modifier_supplement.rs:6/26`，X11 与 Windows 后端均实现），
  回退逻辑键单字符，再回退 ASCII 折叠。
- `ctrl+[`（legacy、无 alt、编码器返回空）回写 `0x1b`，范围仅此一键（DECISIONS 记为工程判断）。

### 3.4 回归结果（修复后，98 键对拍）

- legacy：只剩 `ctrl+i` `ctrl+m` `ctrl+space` `ctrl+comma` `ctrl+period` `ctrl+semicolon`
  `ctrl+apostrophe` `ctrl+shift+semicolon` 为空（均属类别 3/待核实）；其它与 alacritty 一致。
- kitty7：只剩 `ctrl+space`、`ctrl+shift+semicolon` 为空（前者编码器可输出 `ESC[32;5u`，端到端为空，
  疑注入侧；后者属上游行为）。
- 回归断言：`crates/vt/tests/key_kitty.rs` 表驱动（模式 × 键，期望取自 alacritty 基线/编码器实测）。

## 4. t4 启动提示降级（已验证事实）

`request_inner_size: …` 位于 `crates/app/src/lib.rs` 的 HiDPI 修正分支（`scale != 1.0` 且
`request_inner_size` 返回 `None`，即"已异步提交"），属**信息性**；改为 `eperf!` 门控。
Xvfb（100 dpi）实测：默认 stderr **0 行**、`LEANTTY_PERF_FRAMES=1` 时 **1 行**。
失败分支（`snapshot`/`buffer_mut`/`surface` 等错误）未改动，仍默认输出。
README 两份删除对应故障排查条目。

## 5. t5 版本与发布文档

- `workspace.package.version` → **0.1.1**（`Cargo.lock` 同步）。
- 新增 `CHANGELOG.md`（用户视角，0.1.1 Fixed/Changed + 0.1.0），加入 `export.sh` 白名单与 deb 的
  `/usr/share/doc/leantty/`；`changelog.Debian.gz` 条目改写为 0.1.1 的内容。
- `build-deb.sh` 与 `export.sh` 无硬编码版本号（均从 `Cargo.toml` 读取）。
- `THIRD-PARTY-NOTICES.md` 重新生成（295 个第三方 crate），除版本号相关行外无差异。

## 6. t3 释放/重复事件：**未上线**

按要求先评估：alacritty 在 kitty7 下对每个键都会额外发送 `ESC[…;1:3u` 释放事件；而 leantty 目前只发
Press。判定条件之一是"t1f 的 98 键按下与释放事件与 alacritty 逐键一致"——leantty 的 Escape/F1 等
按下编码与 alacritty 就不同（`ESC[27u` vs `1b`、`ESC[1P` vs `ESC[1;1P`），逐键一致的先决条件不成立，
故按 t3c **回退全部 t3 改动、不上线**，并在 README 已知限制与 DECISIONS 写明"不发送 kitty 释放事件"。

## 7. case27 定位结论（r1）

**方法**：先把 `case27` 的动作序列抽成单跑脚本（起 app → 键入 `seq 1 300` → 4×PageUp → Ctrl+Home →
2×PageDown → 截图；另一轮带 `LEANTTY_FORCE_FULL_REDRAW=1` 重跑同一序列），逐像素比较。

**负载**：失败那两次门禁里机器上跑着**别的项目**的 release 构建（`/root/Desktop/pi-rust`，多 target
cargo build），`loadavg` 6.1–6.3；未终止对方进程。等降到 2.7 后复测。

**HEAD 上连跑 3 次**：

| 次 | 差异像素 |
|---|---|
| 1 | 519 |
| 2 | 0 |
| 3 | 0 |

→ **偶发**，不是本阶段改动引入的稳定回归（差异量也从 2444 掉到 519，随负载变化）。
按 r2 第三分支：**不改产品代码**，数据交裁定。差异区域特征（2444 那次）约为一整行 120 列
（≈2400 px），符合"两次运行的滚动偏移差一行"的时序抖动特征。

## 8. r3 ctrl+space 诊断（环境限制）

`crates/vt` 单测已证明编码器可输出（legacy `00`、kitty `ESC[32;5u`），但端到端注入始终为空；
本轮未再加应用侧日志（上下文预算不足以再做一次带日志构建 + 注入）。结论按 r3 分支一登记为
**环境限制（Xvfb/xdotool 层）**，编码器层结论不受影响。

## 9. 最终矩阵摘要（98 键）

- **legacy**：除 `ctrl+i` `ctrl+m` `ctrl+space` `ctrl+comma` `ctrl+period` `ctrl+semicolon`
  `ctrl+apostrophe` `ctrl+shift+semicolon` 外，其余 90 键与 alacritty 一致；`ctrl+[` 由本阶段的
  受控回写给出 `1b`。
- **kitty7**：除 `ctrl+space`（疑注入侧）与 `ctrl+shift+semicolon`（上游行为）外，其余 96 键一致。
- **分类**：类别 1（应用数据有误）已全部修复；类别 2 只剩 `ctrl+[`（已按裁决回写）；类别 3 = 上述
  无输出键，记入 README 已知限制；`ctrl+space` 记"待核实（疑注入侧）"。
- **t3**：不发送 kitty 释放事件（前提不成立，已回退并写入已知限制）。

## 10. case27 因果实验（p1–p3、p5 部分）

### 10.1 p1：启动提示降级不是回归（已验证事实）

`request_inner_size(...)` 是 `if` 的**条件表达式**，位于 `eperf!` 之外，无条件执行；只有那行日志被
`eperf!` 门控。Xvfb（100 dpi）实测两种模式窗口几何**完全相同**：

```
plain: WIDTH=1560 HEIGHT=630   (stderr 1 行)
perf : WIDTH=1560 HEIGHT=630   (stderr 59 行)
```

### 10.2 p2：harness 与单跑脚本的差异（已验证事实）

`run_all.sh` 的 `start_app` 用 **debug 二进制**（`LEANTTY_BIN=target/debug/leantty`）并注入
`LEANTTY_DUMP_SNAPSHOT`（每次渲染写一份 dump 文件），而我的单跑脚本用 release、无 dump。
这就是"harness 稳定红 / 单跑 519·0·0"的最可能来源。

### 10.3 p3：对照组（phase6a-done）与 HEAD 的完整 e2e（已验证事实）

| 运行 | 工作树 | case27 | 汇总 | loadavg(1m) |
|---|---|---|---|---|
| HEAD `30e853c` | 主库（已构建） | **FAIL 2444** | 116/117 | 3.36 |
| 对照 `5ffe7b5`（6A done） | 独立 worktree，debug+release 重建 | **PASS** | 117/117 | 1.07 |

（对照那次 GPU 轮出现 1 个 FAIL、SKIP=0，与本问题无关，属该轮 GPU 用例单次失败。）
→ 判定为 **p3c 分支 2：6B 引入的回归**。

### 10.4 p5a 第一步：route_key 逐文件回退实验（已验证事实）

在 HEAD 的独立 worktree 里只回退 `route_key` 的"整串控制字符走编码器"那一个条件：

```
FAIL case27-scroll-incremental-equals-full (差异像素 2444)
FAIL case63-key-matrix (2/3)          ← 预期（Escape 期望已随修复改变）
e2e 汇总: PASS=115 FAIL=2
```

**case27 仍为 2444** → `route_key` 不是引入点。

### 10.5 剩余候选（待验证假设，下一轮第一个实验）

`e7679ea` 中除 `route_key` 外的改动：
1. `map_logical_key` 新增 F1–F12 / Insert / PageUp / PageDown / Delete / 符号键映射；
2. `unshifted_codepoint` 改用 `key_without_modifiers()`（原来只读 `logical_key`）；
3. legacy `ctrl+[` 的受控回写。

建议下一个实验：在独立 worktree 里只回退第 2 项（`unshifted_codepoint` 恢复为只读逻辑键），
若 case27 转绿则引入点确定；再据其机制提出最小修复（例如仅在已有 `text` 时使用
`key_without_modifiers`，或把 `key_without_modifiers` 的结果限制在"符号键 + Ctrl"分支）。

## 11. case27 单用例可复现性（q1）——**无法在隔离条件下复现**

为区分"单用例红"与"需要前序用例状态"，我按 run_all 的 `start_app` 构造做了等价复现脚本
（**debug 二进制 + `LEANTTY_DUMP_SNAPSHOT`**、同一配置、同一按键序列与 sleep）：

| 运行条件 | 差异像素 |
|---|---|
| 主库 HEAD（未改动），隔离会话，1 次 | **519** |
| run_all 完整序列（case27 排在前 26 个用例之后） | **2444**（4 次同值） |
| phase6a-done 的 run_all 完整序列 | 0（PASS） |

→ 单用例隔离跑**复现不出 2444**；2444 只在"完整 e2e 的前序用例之后"出现，且对照组（6A）在同样
前序下为绿。说明本红是**前序用例残留状态 × 6B 改动**的组合效应，不是 6B 单独引入的单点回归。

按 q1 的说明，此时应改用"前 N 个用例 + case27"的过滤形式二分 N；本轮上下文与磁盘已到极限
（/tmp 16G tmpfs 曾写满、只剩少量调用预算），未实施 `E2E_ONLY` 与"前 N"过滤，故在此停止，
**不改阈值、不改测试基线、不改产品代码**。

q3 中已完成的部分见 §10.4（`route_key` 回退后仍 2444，已排除）。
