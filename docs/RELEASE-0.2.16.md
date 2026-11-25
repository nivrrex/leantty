# leantty 0.2.16

配置文件的读取顺序与 leantty-zig 对齐。

## 原来的问题

| 情况 | 修复前 | 修复后（与 Zig 一致） |
|---|---|---|
| 便携包（配置文件放在 exe 同目录） | **永不读取** | 用户没有自己的配置时读取 `leantty.toml` |
| Windows 上 `APPDATA` 缺失（某些 `cmd.exe`/服务环境） | 直接报"APPDATA 未设置"，整份配置不生效 | 依次退到 `%USERPROFILE%\.config\leantty\leantty.toml`、`HOME/.config/…` |
| 默认路径文件不存在 | 仍返回该路径（行为正确） | 不变，但会先检查便携配置 |

## 现在的顺序

1. `--config-file <路径>`（显式指定时必须存在可读，否则报错退出 1）；
2. `LEANTTY_CONFIG`（空值视为未设置）；
3. 平台默认路径，**仅当该文件存在**：
   - Linux：`$XDG_CONFIG_HOME/leantty/leantty.toml` → `$HOME/.config/leantty/leantty.toml`
   - Windows：`%APPDATA%\leantty\leantty.toml` → `%USERPROFILE%\.config\leantty\leantty.toml`
4. 用户没有配置时，用**可执行文件同目录的 `leantty.toml`**（便携包）；
5. 以上都不存在：交回默认路径，启动时打印"读不到该文件，使用默认配置"。

用户自己的配置**永远**不会被便携配置盖掉——这条在 Zig 侧有明确注释（"否则改 ~/.config 会突然失效"），
这里用同一语义。

## 实现

- `crates/config/src/lib.rs`：新增 `config_base_dir()`（XDG → 按平台）、`portable_config_path()`、
  纯函数 `choose_config_path()`（可测的组合逻辑）与对齐后的 `resolve_config_path()`。
- 文档：`README.md` 的"配置"段与两份模板（`config/leantty.linux.toml`、`leantty.windows.toml`）
  都写了这条顺序。

## 测试

- 单测 `config_path_priority_matches_zig`：7 种组合（CLI 最高 / env 次之 / 默认存在 / 默认不存在退便携 /
  都缺交回默认 / 无基准目录只剩便携 / 彻底没有则报错）。
- 端到端（真实进程，`HOME`/`XDG_CONFIG_HOME` 指向临时目录）：

  | 场景 | 期望 | 实测背景色 |
  |---|---|---|
  | 只有 exe 旁的 `leantty.toml`（#123456） | 用便携配置 | `srgb(18,52,86)` ✓ |
  | 同时有 `~/.config/leantty/leantty.toml`（#aa5500） | 用户配置优先 | `srgb(170,85,0)` ✓ |
  | 再加 `LEANTTY_CONFIG=env.toml`（#005500） | 环境变量最高 | `srgb(0,85,0)` ✓ |

## 验证

- `cargo nextest run --all`：254 passed / 1 skipped；clippy `-D warnings` 0 告警；release 0 告警。
- `scripts/gates.sh`：10 步 PASS（下文"资产"在打包后补哈希）。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.16_amd64.deb` | 3,529,404 B | `09233692501d54ae52dea9108442dafddb1b581ee2b71b9c4f2f84bb1103b2b1` |
| `leantty-0.2.16-windows-x86_64.zip` | 5,556,849 B | `e83e07892394e56653bdfcf8a814edbf4d867c7f535df6da4346f2ae563d999f` |

## 未验证

- Windows 实机（`%APPDATA%` 与 `%USERPROFILE%` 分支只做了单元测试与交叉编译检查，未在 Windows 上跑真实进程）。
