# 今晚测试包 kit v1（不含 GPU 构建）

一页说明。目标机器：有 GPU 的物理 Windows 电脑，只用一个晚上。

## 前置条件

- Windows 10 1809 及以上（x64）；无需安装任何东西，不解压到系统目录也可以。
- 建议**物理登录**（不要用远程桌面）：RDP 会话下 GPU 数据无意义，脚本会自动跳过 GPU 阶段并在 summary 顶部标注。
- 解压到任意**本地**目录（路径含空格、中文都可以，脚本用相对路径）；不要放在网络盘上。
- 需要能使用 PowerShell（系统自带 5.1 即可）。

## 命令

```
powershell -NoProfile -ExecutionPolicy Bypass -File run-tonight.ps1
```

常用变体：

```
# 冒烟试跑（约 3–5 分钟，P3 浸泡缩到 1 分钟）
powershell -NoProfile -ExecutionPolicy Bypass -File run-tonight.ps1 -Quick

# 自定义浸泡时长（默认 5 分钟；总时长约 = 8 分钟 + 浸泡分钟）
powershell -NoProfile -ExecutionPolicy Bypass -File run-tonight.ps1 -SoakMinutes 10

# 跳过浸泡
powershell -NoProfile -ExecutionPolicy Bypass -File run-tonight.ps1 -SkipSoak
```

产物：`results\tonight-<时间戳>.zip`（内含 `summary.md`、`summary.json`、`shots\`、`logs\`、
`environment.json`、`idle-cpu.json`、`soak-*.json`、以及 dxdiag 输出）。**请把这个 zip 回传。**

## 预计耗时

| 阶段 | 内容 | 预计 |
|---|---|---|
| P0 | 环境采集（含 dxdiag，最长 90 s） | 1–2 分钟 |
| P1 | CPU 基线：5 次启动、固定内容截图、30 s 空闲内存、两次吞吐（30 万行）、20 次缩放、最小化/遮挡恢复 | 6–10 分钟 |
| P2 | GPU（本构建在 Windows 上没有 GPU 后端 → 自动记 N/A） | ≈0（若为 GPU 构建：3–8 分钟） |
| P3 | 浸泡（默认 5 分钟/后端，每 30 s 采样 + 输出/缩放/1 MB 粘贴） | 6 分钟 |
| P4 | 汇总与打包 | <10 秒 |

合计：**默认参数约 15–20 分钟**；`-Quick` 约 3–5 分钟。

## 规则（脚本已遵守）

- 不修改系统设置、注册表、全局环境变量、防火墙；不联网；只在 kit 目录内读写。
- 只结束**自己启动的** `app.exe`（按 PID）；**不触碰** `OpenConsole.exe`、`conhost.exe`、WezTerm、pwsh 等任何进程。
- 每个阶段独立 try/catch 且有时限；某阶段失败会记进 summary，后面的阶段照常跑，最终一定生成 zip。
- `[perf]` 日志用直接文件句柄重定向（内部 `cmd /c "... 2>log"`），并设 `APP_PERF_FRAMES=1`。

## 3 条手动项（脚本做不了）

1. **窗口拖到另一块显示器 / 改变系统缩放比例**：把窗口拖到副屏，再把系统缩放改成 125% 或 150%，看字符是否发虚、光标是否错位，记下现象。
2. **GPU 观感与功耗**：如果 summary 里 P2 不是 N/A，用 GPU 后端跑一段 `dir /s C:\Windows` 之类的滚动输出，留意是否掉帧、风扇是否明显加速；用任务管理器截图 GPU 占用。
3. **真实中文与输入法**：在中英混排下用输入法打字（激活预编辑行），确认候选窗位置正确、上屏不错位；再用 Ctrl+Shift+C 复制一段含汉字的选区，粘到记事本核对内容。

把现象写在一段文字里，和 zip 一起回传即可。
