# leantty 0.2.17

本版本以 `rel-0.2.8`（0.2.16 发布线）为基线，集中修复启动、PTY 读写与退出生命周期。

## 修复

- 启动失败时先唤醒事件循环再返回，避免窗口或事件循环残留。
- PTY 读取遇到 `Interrupted` 时重试；其他读错误在退出前报告。
- Unix 子进程退出后等待读线程收尾，避免尾部输出丢失。
- 登录 shell 信息不可用时回退到现有 shell 选择逻辑。
- Unix PTY 主端改为非阻塞；写线程分块写入并等待可写。
- 粘贴阻塞期间收到 Ctrl-C 时丢弃在途粘贴、清空 PTY 输入队列，并中断前台进程组。
- 隔离 Unix 专属 PTY 控制路径，恢复 Windows 目标编译与 clippy。

## 验证

- `cargo nextest run --all`：259 passed / 1 skipped；doctest 通过。
- debug/release clippy：0 告警；release 构建：0 告警。
- `scripts/gates.sh`：10/10 通过；E2E 136 passed / 0 failed，GPU 轮 10 passed / 0 failed。
- Windows GNU 目标 `cargo check --workspace --tests` 与 clippy：通过。
- Windows 发布 exe 在 Wine 11 下执行 `--version` 与 `--help`：通过，版本为 0.2.17。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.17_amd64.deb` | 3,542,532 B | `ee70c2aea3b4f9f01a198cfa082f8072a982e53a429084e55ab3396b29e2a06c` |
| `leantty-0.2.17-windows-x86_64.zip` | 5,561,116 B | `c41d0d290dc7b5eed911043ab1f3c45e12f3bae950871e2b966067ebbd3488bc` |

## 未验证

- 未在真实 Windows 桌面验证窗口、IME、剪贴板和 ConPTY 行为；Wine 仅验证了命令行入口。
- deb 未安装到系统，仅解包并执行其中二进制；`lintian` 未安装，未运行。
