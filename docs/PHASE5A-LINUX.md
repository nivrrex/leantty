# PHASE5A-LINUX：改名 leantty + 命令行参数 + 图标与窗口标识 + 发布材料

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 改名 | 二进制 `leantty`、包名 `leantty-*`、环境变量 `LEANTTY_*`、配置目录 `leantty/leantty.toml`、窗口标题回退串；黄金哈希 **507485fb7945fb8f 不变**；e2e 113/113；tag `phase5a-t1` |
| t2 命令行 | `--help/-V/--config-file`，纯函数解析 + 8 单测；退出码 0/2/1；e2e case56..61（含配置优先级对拍）；tag `phase5a-t2` |
| t3 图标 | SVG → 8 尺寸 PNG + ICO + 128x128 RGBA；`Icon::from_rgba`；WM_CLASS/app_id = leantty；单测 + e2e case62；tag `phase5a-t3`（**该 tag 的 e2e 实际中断，见 §3.4，已另建 `phase5a-t3b`**）|
| t4 Windows 控制台 | `should_free_console` 纯函数 + `GetConsoleProcessList`/`FreeConsole`（cfg(windows)），windows-gnu check 通过；Windows 实机未验证 |
| t5 发布材料 | Cargo 元数据（version/license/publish/description/strip）、MIT LICENSE、THIRD-PARTY-NOTICES（295 crate）、两份配置模板、desktop/cmd、双语 README、BUILDING.md |
| t6 收尾 | gates 全过、e2e 全过、tag `phase5a-done` |

## 1. t1 改名

改动面（机械改名，102 个文件）：

- 二进制：`app` → `leantty`（`[[bin]] name = "leantty"`；库目标名保留 `app`）。
- 包名：`app/config/vt/pty/render` → `leantty-app/leantty-config/leantty-vt/leantty-pty/leantty-render`；
  各库显式写 `[lib] name = "<原短名>"`，因此内部 `use config::` 等 import 无需改动。
  **否决**"连 lib crate 名一起改"：几百处 import 变动，收益为 0，回归风险高。
- 环境变量：全部 `APP_*` → `LEANTTY_*`（含 debug 专有项），不保留旧名；`WGPU_*` 未动。
- 配置路径：由 `app_name()`（`args[0]` 的 file_stem）派生，二进制改名后自动变为
  `$XDG_CONFIG_HOME/leantty/leantty.toml` 与 `%APPDATA%\leantty\leantty.toml`；回退串改为 `leantty`。
- 脚本（e2e/构建/Windows ps1/cmd）：`target/release/app` → `target/release/leantty`、`cargo -p app`
  → `-p leantty-app`、shell 变量名同步。
- 历史阶段报告与 `docs/DECISIONS.md` 历史条目**不改写**；README、`docs/PROJECT-SUMMARY.md` 已同步。

验收：`scripts/gates.sh` 全过；`frame_bytes_identical` 黄金哈希 **507485fb7945fb8f** 与字体指纹
不变；e2e 113/113；`cargo check --target x86_64-pc-linux` 未涉及，Windows 目标 0 error/0 warning；
全仓除历史文档外无 `APP_` 旧名残留。

## 2. t2 命令行参数

- `crates/app/src/cli.rs::parse_args(args_os)`：纯函数，`OsString` 处理路径（非 UTF-8 路径保真，
  单测覆盖）；`--config-file X` 与 `--config-file=X` 等价；`--config` 保留为隐藏别名（已合并的
  Windows 脚本在用）。
- 退出码：帮助/版本 0；用法错误（未知选项、缺值、多余位置参数）2 且 stderr 一行 + `Try --help`；
  显式 `--config-file` 不可读/不可解析 → 1，stderr 含路径，不开窗口。
- 解析先于 `App::new` 与事件循环 → 无 `DISPLAY` 也可用（case56/57 实测 rc=0）。
- 优先级：`--config-file` > `LEANTTY_CONFIG` > 默认路径；环境变量与默认路径读不到时仍是
  "警告 + 默认配置启动"（既有行为不变）。
- **否决**把 `--font`/`--font-size` 定为正式选项：帮助文本按任务只列三项，且未知选项一律退出 2；
  这两个覆盖仍由 `LEANTTY_FONT_FAMILY`/`LEANTTY_FONT_SIZE` 提供。

e2e（全部有证据）：

```
[c56] --help 无 DISPLAY → rc=0，含用法/三选项/LEANTTY_CONFIG/默认路径，stderr 空
[c57] --version 无 DISPLAY → rc=0，输出='leantty 0.1.0'
[c58] 未知选项 → rc=2，stderr 含 unknown option 与 Try --help
[c59] --config-file 缺值 → rc=2，stderr 含 requires a value
[c60] 不存在文件 → rc=1，stderr 含路径
[c61] 优先级 → 命令行键警告=1 环境键警告=0（反向仅环境变量时=1）
PASS cli-args (6/6)
```

## 3. t3 图标与窗口标识

### 3.1 资源生成

`scripts/gen-icons.sh`（rsvg-convert，缺失时 ImageMagick convert）→
`assets/icon/png/leantty-{16,24,32,48,64,128,256,512}.png`、`assets/icon/leantty.ico`（7 尺寸）、
`crates/app/assets/window-icon-128.rgba`（65536 字节）。脚本内置断言：各尺寸非空、含背景色
`#14181f`、具备前景图形（≥32px 含主色 `#5eead4`）。

### 3.2 窗口接入

`window_icon()`：`include_bytes!` + `Icon::from_rgba(128,128)`，失败只打用户可见警告。
`WM_CLASS`/`app_id` 用 winit 0.30 的平台扩展（`WindowAttributesExtX11::with_name` 与
`WindowAttributesExtWayland::with_name`，两者同名，需全路径限定）。单测
`icon_tests::window_icon_rgba_is_128x128` 断言长度与"非全透明"。

实测：

```
WM_CLASS(STRING) = "leantty", "leantty"
_NET_WM_ICON(CARDINAL) = Icon (128 x 128)
PASS case62-window-ident
```

### 3.3 生成脚本的断言取舍（工程判断）

16/24 px 缩放后青绿与底色混合（16px 实测最接近的像素是 `#326c68`），因此 ≤24px 只断言"存在
与底色通道差 >30 的前景像素"，≥32px 才断言精确主色 —— 不用经验色值硬凑。

### 3.4 门禁缺口（已验证事实，已修）

`run_all.sh` 开头 `set -u`；新加的 case62 误用改名前的 `$APP_REL`/`$APP_PID`，在打印 `e2e 汇总`
之前就退出；gates 第 5 步只检查退出码与 `^FAIL` 行 → **半途中断的 e2e 仍判 PASS**，
`phase5a-t3` 就是在这个状态下打出的。处理：不改写该 tag（禁 force），修两处后另建
`phase5a-t3b`；gates 增加"必须存在 e2e 汇总行"的断言，杜绝此类静默中断。

## 4. t4 Windows 控制台释放（待验证假设）

- `should_free_console(u32) -> bool`：仅 `==1` 为真（0 表示查询失败、≥2 表示还有别的进程共享
  控制台，都不动）。单测 4 个断言。
- FFI：`unsafe extern "system" { GetConsoleProcessList; FreeConsole }`，先以
  `GetConsoleProcessList(null_mut(), 0)` 取进程数，为 1 才 `FreeConsole()`；任何失败一律忽略。
  SAFETY 注释写明两个调用的参数语义与不持有指针。
- 调用点：`main.rs` 中参数解析成功、非 help/version、创建事件循环之前（`cfg(windows)`）。
- `cargo check --target x86_64-pc-windows-gnu --workspace --tests` 0 error/0 warning。
- **未在 Windows 实机验证**（本阶段只写代码）。推翻条件：ConPTY 子进程启动异常、Windows e2e
  变红、或从已打开的 cmd 窗口启动导致对方窗口消失 → 整段回退，仅保留 `should_free_console`。

## 5. t5 发布材料

- Cargo：`[workspace.package] version = "0.1.0"`, `license = "MIT"`, `publish = false`,
  `description`；各 crate 用 `.workspace = true` 继承；`[profile.release] strip = "symbols"`。
- `LICENSE`：MIT，署名 "leantty contributors"。
- `scripts/release/gen-notices.sh`：`cargo metadata`（宿主 + `x86_64-pc-windows-gnu`）→
  `THIRD-PARTY-NOTICES.md`（**295 个第三方 crate**，含各许可证文本首段、libghostty 说明与随附
  ConPTY 的许可证全文）。GPL 判定按 SPDX `OR` 语义：`r-efi` 为
  `MIT OR Apache-2.0 OR LGPL-2.1-or-later`，可选用 MIT，不构成 GPL 义务；只有"所有备选都含 GPL"
  或"无许可证信息"时脚本以 1 退出。
- 配置模板：`config/leantty.linux.toml`、`config/leantty.windows.toml`（`git mv` 自 `examples/`，
  `examples/` 已删除）。英文注释、逐字段默认值、通用字体候选（Linux `DejaVu Sans Mono` +
  `Noto Sans CJK SC`；Windows `Consolas` + `Microsoft YaHei`），无个人/机器相关设定。
  验收：config 单测载入两份模板零未知字段告警；不存在的字体名只警告不崩溃（实测进程存活、
  stderr 仅 2 条既有提示）；Linux 模板经 `--config-file` 在 :99 启动 → **stderr 仅 1 行**
  （`request_inner_size: 尺寸请求异步提交…` 的通用提示），无配置相关告警。
  与任务书"stderr 为空"的字面差异：这一行是既有的窗口尺寸提示，与模板无关，本阶段不改。
- `packaging/linux/leantty.desktop`（Name/Exec/Icon=leantty、Terminal=false、
  Categories=System;TerminalEmulator;、StartupWMClass=leantty）、
  `packaging/windows/leantty-portable.cmd`（CRLF，`start "" "%~dp0leantty.exe" --config-file "%~dp0leantty.toml"`）。
- `README.md`（英文）与 `README.zh-CN.md`：面向最终用户，章节为简介与特性 / 安装 / 快速开始 /
  配置 / 命令行选项 / 键鼠 / 渲染后端 / Windows 说明 / 已知限制 / 故障排查 / 许可证；
  无阶段编号、开发日志、agent、本机路径、主机名、个人信息、测试机型号。GPU 只写"在 AMD 与
  NVIDIA 的 Vulkan 环境下测试过"，Windows DX12 写"未在真实硬件验证"。
- `docs/BUILDING.md`：固定 Zig 0.17.1、ghostty commit `0b8b9984…`、libghostty-rs 0.2.1、
  三个构建环境变量与步骤、图标与 notices 的生成命令，不含本机路径。

## 6. 测试与门禁

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **204 passed / 0 failed** |
| e2e | 见 §7（含新增 case56..62）|
| `cargo check --target x86_64-pc-windows-gnu --workspace --tests` | 0 error / 0 warning |
| `cargo fmt --all --check` / clippy 两配置 | 全过 |
| `frame_bytes_identical` | 507485fb7945fb8f（不变）|

## 7. 未解决 / 待验证

1. Windows 控制台释放（t4）**未在实机验证**（待验证假设，见 §4 的推翻条件）。
2. 模板启动 stderr 仍有 1 条窗口尺寸提示（既有行为，非本次引入）。
3. `phase5a-t3` 标记的 e2e 不完整，正确的检查点是 `phase5a-t3b`。
