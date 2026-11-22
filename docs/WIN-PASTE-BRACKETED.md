# Windows 多行粘贴：ConPTY 丢括号粘贴标记（0.2.12）

## 现象（Windows 侧实测，转述）

`leantty-rust.exe` 里跑 `pi`（全屏 TUI，启动发 `ESC[?2004h`），粘贴 7 行文本：
期望整块进输入框或显示 `[paste #1 +7 lines]`；实际只进第一行并立即提交，其余逐行提交。
同一台机器上 `leantty-zig 0.1.8` 表现完全相同。

## 已取证的核心事实（Windows 侧，字节级）

子进程侧探针（`stdinhex.exe`，VT 输入模式，dump stdin 原始字节）收到的是：

```
6C 69 6E 65 20 30 31 ... 74 65 73 74  0D 0D
6C 69 6E 65 20 30 32 ... 74 65 73 74  0D 0D
```

客户端侧日志确认发出的是**一次** `WriteFile`、187 字节，含 `ESC[200~` 与 `ESC[201~`。结论：

1. 收件箱 conhost（本机 `10.0.19041.4522`）的输入解析器**不认**这两个 CSI，标记被当散键丢弃；
2. 它把每个 `CR` 翻成**两个** `CR` → 每行多一次回车 → 逐行提交。

未验证：Rust 版当时是否真的发出标记（该结论只能由 Windows 实机日志确认）。

## 本仓库的判断与改动（0.2.12）

- **标记发送**：`is_bracketed_paste()` 读的是 VT 模式的全局状态（与在哪个屏幕无关），
  `normalize_paste` 会在正文两端加 `ESC[200~ … ESC[201~`；`scripts/e2e/paste_bracket_probe.sh`
  在 Linux 上实测子进程确实收到且只收到一对标记（含 370 KB 分块场景）。
- **换行选择（本次修复）**：`paste::paste_newline(bracketed, windows)`。
  Windows + 括号粘贴用 `LF`，其余用 `CR`：
  - 标记被 conhost 丢弃时，`LF` 不会被加倍，前端把整段当文本插入而不是逐行提交；
  - 标记正常到达时（随包 OpenConsole），`LF` 在括号粘贴内同样只是换行文本；
  - 非括号粘贴（`cmd`/`powershell` 未开 `?2004`）保持 `CR`，多行命令仍逐行执行；
  - Unix 不受影响（`cfg!(windows)` 注入，测试用参数覆盖两平台）。
  孤立 `\r` 在 LF 模式下也归一为 `\n`（conhost 同样会把它翻成两个 CR）。
- **标记过滤**：正文里 `ESC[200~` 与 `ESC[201~` 两个都滤掉（此前只滤结束标记，
  伪造的起始标记会让前端误判为嵌套粘贴）。
- **分块不重复标记**：包裹发生在分块之前，`paste_chunks` 只切正文；新增单测
  `chunked_bracketed_paste_has_exactly_one_marker_pair`（2 MiB）锁定这条不变量。

## conpty.dll 旁加载：已具备，不是待办

`portable-pty 0.8.1`（`src/win/psuedocon.rs::load_conpty`）先 `LoadLibrary("kernel32.dll")`，
再尝试 `LoadLibrary("conpty.dll")`，后者按 Windows 默认搜索顺序**先查程序目录**。
所以随包发布 `conpty.dll` + `OpenConsole.exe`（0.2.2 起一直如此）时用的就是新版 OpenConsole，
而不是收件箱 conhost。发布 zip 的 `leantty.exe` 里能查到 `conpty.dll` / `OpenConsole.exe` 字样。

反过来说：**如果 Windows 实机仍然丢标记，先确认这两个文件确实在 `leantty.exe` 同目录**
（`crates/pty` 里已有"只有 conpty.dll、缺 OpenConsole.exe"的启动警告）。

## Linux 侧验证（可重复执行）

```
sh scripts/e2e/paste_bracket_probe.sh
```

实测输出（0.2.12）：

```
small: 收到 103 B | 200~ x1 201~ x1 | CR 7 LF 0 CRCR 0 | 正文 91 B | 源 91 B
big:   收到 370012 B | 200~ x1 201~ x1 | CR 20000 LF 0 CRCR 0 | 正文 370000 B | 源 370000 B
```

`small` 是 7 行文本（对应报告里的失败场景），`big` 是 370 KB（跨 6 个 64 KiB 写块）。
两者都只有一个标记对、每个换行只有一个 CR、正文字节数与源完全相等。

## 仍未验证（需要 Windows 实机）

1. Windows + `pi` 粘贴 7 行：是否整块进输入框（预期：是，因为正文里不再有 CR）；
2. Windows + `cmd` 多行粘贴：是否仍逐行执行（预期：是，未开 `?2004` → CR）；
3. 实机 conhost 是否真的丢标记——用同一份探针在 Windows 上 dump 子进程 stdin 字节即可判定；
4. >300 KiB 粘贴在 ConPTY 输入管道上的表现（Linux 侧已验证分块不重复标记，Windows 管道行为未测）。

## 排障提示

- 粘贴后若前端"卡在粘贴中"（后续键入不执行）：说明起始标记已写出、结束标记没到。
  先看 `[perf] ctrl_c_restore_paste_end` 有无打印（Ctrl-C 会补发结束标记）。
- 若出现隔行空行：检查 `CRCR` —— 正文里混入了 `\r`（本版本已在 LF 模式下归一）。
