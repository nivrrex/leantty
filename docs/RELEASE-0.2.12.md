# leantty 0.2.12

修复 Windows 上多行粘贴被逐行提交（ConPTY 输入解析丢掉括号粘贴标记、并把每个 CR 翻成两个）。

## 修复

- **Windows 多行粘贴**：应用开了括号粘贴（`ESC[?2004h`）时，粘贴正文在 Windows 上用 `LF` 换行，
  不再把每行变成一次回车。原因（Windows 侧字节级取证）：收件箱 conhost（如 `10.0.19041.4522`）
  的输入解析器不认 `ESC[200~` / `ESC[201~`，把两个标记当散键丢弃，同时把每个 `CR` 翻成两个 `CR`，
  于是每行都多一次回车 → 前端逐行提交。标记仍照常发送：对端真收到时（随包 OpenConsole），
  `LF` 在括号粘贴内就是普通换行；对端丢标记时，`LF` 不会被加倍。
  非括号粘贴（`cmd` / `powershell` 未开 `?2004`）保持 `CR`，多行命令仍逐行执行。孤立的 `CR` 一并归一。
- **粘贴正文可注入伪造起始标记**：此前只过滤 `ESC[201~`，正文里的 `ESC[200~` 会让前端误判为嵌套粘贴；
  现在两个标记都从正文剔除。

## 测试

- 新增单测：`windows_bracketed_paste_uses_lf`、`unix_bracketed_paste_keeps_cr`、
  `windows_bracketed_paste_normalizes_lone_cr`、`pasted_markers_are_filtered_from_content`、
  `chunked_bracketed_paste_has_exactly_one_marker_pair`（2 MiB：分块前后只有一对标记、拼接逐字节等于原文）。
- 新增手动端到端探针 `scripts/e2e/paste_bracket_probe.sh`（子进程开 `?2004` 并 dump stdin 原始字节）：

  ```
  small: 收到 103 B | 200~ x1 201~ x1 | CR 7 LF 0 CRCR 0 | 正文 91 B | 源 91 B
  big:   收到 370012 B | 200~ x1 201~ x1 | CR 20000 LF 0 CRCR 0 | 正文 370000 B | 源 370000 B
  ```

## 未验证（需 Windows 实机）

1. Windows + `pi` 粘贴 7 行是否整块进入输入框；
2. Windows + `cmd` 多行粘贴是否仍逐行执行；
3. 实机 conhost 是否真的丢标记（用同一探针 dump 子进程 stdin 即可判定）。

详见 `docs/WIN-PASTE-BRACKETED.md`。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.12_amd64.deb` | 3,522,748 B | `382829d6070de56bdede28ec0f2f4bd3dcf18a6ea42c9ad76f36493bc900dacd` |
| `leantty-0.2.12-windows-x86_64.zip` | 5,541,503 B | `75bd64560f795115e7369587ca04a81a605e26fbf887ba892e7a80c45aad9e28` |
