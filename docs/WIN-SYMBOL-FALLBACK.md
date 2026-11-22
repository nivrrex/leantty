# Windows 符号缺失修复：默认字体回退链

- 日期：2026-11-22（系统时间）
- 分支：`win`（已合并 `rel-0.2.8`，即 0.2.10 基线）；未动 `main`。
- 结论分级：**已验证事实**（实测/命令输出）/ 工程判断 / 待验证假设。

## 1. 现象与根因（已验证）

同一台 Windows 机器：Zig 版能显示制表、箭头、几何、装饰、盲文、数学运算符；Rust 版部分空白。

根因不止"没有默认链"，实测发现两处叠加：

1. `crates/app/src/lib.rs` 原 `DEFAULT_SYMBOL_FALLBACKS = ["Symbola", "DejaVu Sans", "Noto Sans Symbols2"]`
   是 Linux 名单，且**无条件追加**到配置之后。本机实测 `Symbola`、`Noto Sans Symbols2` 均不存在
   （fontdb 解析为 `None`），只剩 `DejaVu Sans`：
   - `DejaVu Sans` 对语料的覆盖：制表 76/76、方块 32/32、箭头 112/112、几何 96/96，但
     杂项符号 189/256、装饰 174/192、盲文 256/256、数学 256/256、emoji 13/31、CJK 0；
   - 主字体 `Consolas`：箭头 7/112、几何 20/96、盲文 0、全角 0、emoji 0。
2. `crates/render/src/fontset.rs` 的 `face_has()` 按真实 cmap 判定（0.2.9 起有意为之，避免 crossfont
   自动换 face 导致字重不一致），因此链上没有的码位就是空白，**没有任何兜底**。

## 2. 改动（4 处）

| 文件 | 改动 |
|---|---|
| `crates/render/src/fontset.rs` | 新增 `DEFAULT_FALLBACK_FAMILIES`（Windows = 4 项；非 Windows 保持原 3 项）、`default_fallback_families()`、`effective_fallback()`；`FontSet::load` 改用 `effective_fallback`，`[cov]`/perf 计数随之反映实际链 |
| `crates/app/src/lib.rs` | `fallback_chain()`：Windows 下走 `render::fontset::effective_fallback`（**用户配置即最终链，不追加**；配置为空则播种）；非 Windows 保留"配置 + 内置符号兜底（去重）"历史行为 |
| `crates/render/src/fontset.rs` | `load_one()`：`Style::Specific` 失败时退化为 `Style::Description`（取家族默认面）。原因：DejaVu 系常规面样式名是 `Book`，DirectWrite 精确匹配 `Regular` 会 `FontNotFound`（fontconfig 则模糊匹配）。相同处理与 alacritty 一致 |
| `crates/render/src/lib.rs` | CPU 绘制：字形光栅失败（DirectWrite 对空格返回无位图）不再 `continue` 跳过整格，改为跳出 `'glyph` 块后仍绘制下划线/删除线/上划线。修复前 `' ' + underline` 整格无墨迹 |

未改动：`face_has()` 的 cmap 判定（按任务要求保留 0.2.9 行为）；未引入每次取字形扫库（fontdb 仍是懒加载 + 缓存）。

## 3. 默认链顺序与依据（已验证）

最终顺序：

```
主字体（Consolas…，用户配置）
→ Segoe UI Symbol → Segoe UI Emoji → Microsoft YaHei → MS Gothic
```

依据（本机 fontdb + ttf-parser 实测 face 真实 cmap，语料 1693 码位 / 13 区块）：

| face | 制表 | 方块 | 箭头 | 几何 | 杂项 | 装饰 | 盲文 | 数学 | 全角 | Powerline | emoji | CJK-B |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Consolas | 76/76 | 8/32 | 7/112 | 20/96 | 12/256 | 11/192 | 0/256 | 17/256 | 0/94 | 0/20 | 0/31 | 0/16 |
| Segoe UI Symbol | 76/76 | 32/32 | 112/112 | 96/96 | 256/256 | 192/192 | 256/256 | 256/256 | 0/94 | 20/20 | 28/31 | 0/16 |
| Segoe UI Emoji | 0/76 | 0/32 | 112/112 | 96/96 | 124/256 | 66/192 | 0/256 | 10/256 | 0/94 | 0/20 | 31/31 | 0/16 |
| Microsoft YaHei | 76/76 | 18/32 | 8/112 | 20/96 | 5/256 | 0/192 | 0/256 | 41/256 | 94/94 | 0/20 | 0/31 | 10/16 |
| MS Gothic | 76/76 | 22/32 | 91/112 | 80/96 | 108/256 | 160/192 | 0/256 | 242/256 | 94/94 | 0/20 | 2/31 | 11/16 |

- 符号字体前置的理由：`Segoe UI Symbol` 对全部非 CJK 区块基本全覆盖，且是**单色**字形；若把
  `Segoe UI Emoji` 放前面，箭头/几何/装饰会被彩色字形截走（Zig 侧链顺序 CJK 在前的副作用）。
- `Microsoft YaHei` 补全角与常用 CJK；`MS Gothic` 补 YaHei 缺的箭头（8→91）、数学（41→242）与 CJK-B。
- 与 Zig 侧 `src/win32.c` 的链（YaHei → SimSun → Noto Sans CJK SC → Segoe UI Emoji → Noto Color Emoji
  → Segoe UI Symbol → Symbola）在**覆盖集合**上等价，差异只在先后（本机无 SimSun 之外的 Symbola /
  Noto Color Emoji，`SimSun` 覆盖明显弱于 YaHei，故未采用）。

链的并集覆盖：1693 码位中覆盖 1443，缺 250（见 §5）。

## 4. 单测与检查

- `crates/render/tests/font_fallback_default.rs`（新增 5 项，全绿）：
  - `empty_config_seeds_platform_default_chain_in_order`（空配置 → 平台链，Windows 顺序固定）
  - `platform_default_chain_is_exposed_for_callers`（Windows 首项必须是符号字体且含 CJK 字体）
  - `user_config_is_used_verbatim_without_appended_defaults`（用户配置不被追加）
  - `user_config_containing_a_default_family_is_not_extended`
  - `regular_style_loads_fonts_whose_face_is_named_book`（`Style::Specific("Regular")` → Description 回退）
- `cargo test --workspace --no-fail-fast`：**264 passed / 0 failed**（43 个测试二进制）。
  为让 Windows 上真正跑起来，另修了三处测试平台的字体假设（不改断言语义）：
  `font_metric_chain.rs` 改用 `Style::Description`（DirectWrite 拒绝 `Specific("Regular")`）；
  `font_ready.rs` 兜底列表补 Windows 字体；`wide_glyph.rs` 主字体在 Windows 用 `Consolas`
  （`monospace` 是 fontconfig 通用名，DirectWrite 不认）。
- `cargo clippy --all-targets -- -D warnings`：rc=0。
- `cargo build --release -p leantty-app`：rc=0。

## 5. `[cov]` 前后对比（`LEANTTY_FONT_COV_DEBUG=1`，配置无 `font.fallback`）

修复前（基线二进制 = `cloud/rel-0.2.8` 构建）：

```
[cov] 0..3  Consolas Regular/Bold/Italic/Bold Italic      id=Some
[cov] 4     Symbola          id=None   (未安装，被跳过)
[cov] 5     DejaVu Sans      id=Some
[cov] 6     Noto Sans Symbols2 id=None (未安装，被跳过)
```

修复后：

```
[cov] 0..3  Consolas Regular/Bold/Italic/Bold Italic  id=Some
[cov] 4     Segoe UI Symbol  id=Some (ID 299)
[cov] 5     Segoe UI Emoji   id=Some (ID 293)
[cov] 6     Microsoft YaHei  id=Some (ID 172)
[cov] 7     MS Gothic        id=Some (ID 163)
```

## 6. 语料截图（同一份 `glyph-corpus.txt`，130×45，Consolas 13pt）

- 修复前：`vt-verify-win/glyph-run/out/base.png` —— 箭头/几何/杂项/装饰/盲文/数学/全角/Powerline/emoji
  大片空白（连中文区块标题也缺字）。
- 修复后：`vt-verify-win/glyph-run/out/new10.png` —— 上述区块全部有字形，CJK 标题与全角正常。
- 运行脚本：`vt-verify-win/glyph-run/run_glyph_shots.ps1`、`shot_by_pid.ps1`（`PrintWindow` 后台抓图，
  避开前台锁）。

## 7. 仍缺的码位（未装字体，非缺陷）

- Nerd 私有区 `U+F000–F0FF`：缺 248/256（本机未装 Nerd Font；`Segoe UI Symbol` 恰好只有 8 个）。
- CJK 扩展 B：`U+2418C`、`U+2A6CA`（全机只有 `MingLiU-ExtB`/`SimSun-ExtB` 覆盖，链上未包含）。
- 其余 11 个区块：**0 缺失**。

## 8. 未完成/限制（如实记录）

- **Zig 终端对照截图未取得**：本机 `leantty-zig` 的 `-e` 在本轮重建后对 `cmd.exe /c <批处理>` 不生效
  （批处理里的 marker 文件未生成、窗口显示的是默认 `cmd /K` 提示符；`-e ping` 正常），
  `-e py.exe` 会直接崩溃。因此对照只做到"同语料的 Rust 修复前/后截图 + 两套链的覆盖集合实测"，
  Zig 侧逐像素对照属**待验证**。Zig 链的覆盖集合已用同一份 fontdb/ttf-parser 探针实测：
  Zig 链（可用 face = Consolas / Microsoft YaHei / SimSun / Noto Sans CJK SC / Segoe UI Emoji /
  Segoe UI Symbol，`Noto Color Emoji` 与 `Symbola` 未安装）与本轮 Rust 新链**同为 1443/1693**，
  缺失码位集合完全相同（PUA 248 + `U+2418C` + `U+2A6CA`）。

## 9. 交付

- 提交（分支 `win`，未动 `main`、未 force push）：
  - `f3d949c` fix(win): 播种 Windows 默认字体回退链（符号/CJK 不再空白）
  - `47ea9c1` verify(win): 符号回退链语料截图与 [cov] 前后证据
  - `aa7f3c7` chore(win): 移除探索期的临时文件
- 推送：`git push cloud win` → `2c3ff22..aa7f3c7  win -> win`
- `git ls-remote cloud refs/heads/win` = `aa7f3c724dd0fd51cb709129fa7d9c9a0609b973`（与本机 `git rev-parse win` 一致）
