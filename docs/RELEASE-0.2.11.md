# leantty 0.2.11

在 0.2.10 之上合并 Windows 侧的字形回退链修复（`win` 分支）；Linux/macOS 行为不变。

## 修复

- **Windows 符号缺失**：主字体（Consolas 等）缺的码位此前没有任何兜底——内置原来是 Linux 名单
  （`Symbola` / `DejaVu Sans` / `Noto Sans Symbols2`），Windows 上通常只装了中间那个，于是制表、箭头、
  几何、装饰、盲文、数学、全角、CJK 大片空白。现在未配置 `font.fallback` 时按平台播种：
  Windows 为 `Segoe UI Symbol` → `Segoe UI Emoji` → `Microsoft YaHei` → `MS Gothic`，
  Linux/macOS 保持原名单（配置链之后追加）。用户显式配置 `font.fallback` 时以其为准，不追加任何默认项。
- **风格名不是 "Regular" 的字体在 DirectWrite 下加载失败**：例如 DejaVu 的常规面叫 `Book`，按 `Regular`
  精确匹配会 `FontNotFound`；现在退化为按风格描述（style description）加载，取该族默认面，与 alacritty 一致。
- **空格 + 下划线/删除线整格无墨迹**：光栅化返回空位图（DirectWrite 对空格不给位图）时此前会跳过整格，
  连装饰线也不画；现在只跳过字形，装饰线照画。

## 与 0.2.10 的关系

0.2.11 = 0.2.10 + `win` 分支（含 Linux 上的 0.2.10 全部内容）。GPU present 计时、遮挡跳过渲染、
字形覆盖查询（fontdb + ttf-parser）等沿用 0.2.10。

## 验证

- `cargo test --workspace`（Linux）：43 个测试二进制全绿；`cargo clippy --all-targets -D warnings` rc=0。
- Windows：`cargo test --workspace` 264 passed / 0 failed；`[cov]` 由
  `Consolas×4 + Symbola(None) + DejaVu Sans + Noto Sans Symbols2(None)`
  变为 `Consolas×4 + Segoe UI Symbol + Segoe UI Emoji + Microsoft YaHei + MS Gothic`（均解析成功）。
- 语料对照（1965/1693 码位，`scripts/e2e/glyph-corpus.txt`）：11 个符号区块 0 缺失；
  剩余缺失只有 Nerd 私有区 `U+F000–F0FF`（未装 Nerd Font，非缺陷）与 `U+2418C`、`U+2A6CA`（CJK 扩展 B，
  全机只有 `MingLiU-ExtB`/`SimSun-ExtB` 覆盖）。
- 未验证：Zig 版逐像素对照（本机 Zig 构建跑 `-e cmd.exe /c`、`-e py.exe` 均失败或崩溃）；
  两套链的覆盖集合已实测相同（1443/1693，缺集一致）。

## 资产

见 `SHA256SUMS`。
