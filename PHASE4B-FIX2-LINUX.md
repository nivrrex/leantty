# PHASE4B-FIX2-LINUX：字形顶部裁剪取证与修复（附滚动条加宽）

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 结论摘要

| 问题 | 根因 | 状态 |
|---|---|---|
| 汉字/高字形顶部被裁 | `baseline = cell_h - ceil(-descent) - ceil(line_gap)` 把 line_gap **全放在格底**，使 baseline < ceil(ascent) | **已修**：line_gap 居中 + 逐字形平移兜底 |
| 滚动条太窄难命中 | 宽度固定 4px（绘制、条带备份、命中测试三处魔数） | **已放宽到 8px**（三处统一为常量） |

修复前后对比（`glyph_clip` 取证测试，13px）：

| 字体链 | 修复前 | 修复后 |
|---|---|---|
| Consolas（主） | baseline=9，`min_top=-2`，**13 个 ASCII 字符顶部被裁**（共 14 行） | baseline=11，`min_top=0`，0 裁 |
| FreeMono + CJK 回退 | **139 个汉字顶部被裁**（共 272 行） | 0 裁（1 个字形高度 > 格，保顶裁底） |
| Consolas + CJK 回退 | CJK 顶部被裁 | 0 裁 |
| DejaVu / Liberation / Noto（主） | 0 裁（这些字体 line_gap=0，旧公式恰好等于 ceil(ascent)） | 0 裁 |

## 1. 取证（修复前）

测试：`crates/render/tests/glyph_clip.rs`。对 ASCII `0x21–0x7E` 与 **250 个常用汉字**，用与渲染相同的公式计算 `top = baseline - ymin - height`，断言 `top >= 0 && top + height <= cell_h`；同时统计"格上下各加 8px 余量后会被露出的墨迹行数"。

字体矩阵（13px，本机 + Consolas + CJK 回退）：

| 字体 | ascent | descent | line_gap | cell_h | baseline（旧） | min_top（旧） | 被裁字符 | 被裁行 |
|---|---|---|---|---|---|---|---|---|
| DejaVu Sans Mono（主） | 12.07 | -3.07 | 0.00 | 16 | 12 | 1 | 0 | 0 |
| Liberation Mono（主） | 10.82 | -3.90 | 0.00 | 15 | 11 | 1 | 0 | 0 |
| Noto Sans Mono（主） | 13.90 | -3.81 | 0.00 | 18 | 14 | 4 | 0 | 0 |
| FreeMono（主） | 10.40 | -2.60 | 0.00 | 13 | 10 | 1 | 0 | 0 |
| **Consolas（主）** | 9.65 | -3.35 | **2.22** | 16 | **9** | **-2** | **13** | **14** |
| CJK 回退（Noto Sans CJK） | 15.08 | -3.74 | 0.00 | 16 | 由主字体定 | 0 | 0 | 0 |
| CJK 回退 + Liberation（cell_h=15） | 15.08 | -3.74 | 0.00 | 15 | 11 | **-1** | 1 | 1 |
| **CJK 回退 + FreeMono（cell_h=13）** | 15.08 | -3.74 | 0.00 | 13 | **9** | **-3** | **139** | **272** |

被裁字符样例（Consolas）：`$ % ( ) @ [ ] f i j { |`（`|` 最深 -2）；FreeMono+CJK：`的 是 了 我 不 人 在 他 有 这 个 上 …`（-1 ~ -2）。

**GPU 侧**：`glyph_top_not_clipped_gpu` 用 40 个汉字渲染，统计"贴顶格第一行有墨迹"的格数，修复前有贴顶格，修复后 **0/40**。

## 2. 根因

`crates/render/src/lib.rs` 的 `baseline_px`（4B-fix1 引入）：

```rust
// 修复前
let descent_px = (-lm.descent).ceil().max(0.0);
let gap_px = lm.line_gap.ceil().max(0.0);
baseline = (cell_h as f32 - descent_px - gap_px).clamp(0.0, cell_h as f32) as i32;
```

`cell_h = ceil(ascent - descent + line_gap)`，把 `line_gap` 全减到 baseline 上，等价于**把 line_gap 全部塞到格底**。当 `line_gap > 0`（Consolas 为 2.22）时：

```
baseline = cell_h - ceil(-descent) - ceil(line_gap) = ceil(ascent) - ceil(line_gap) + 残差
∴ baseline < ceil(ascent)     ← 差 1~2px
```

字形顶边 `top = baseline - ymin - height` 因此落到格顶之上被 `blit` 的 cell 裁剪切掉。CJK 回退字体 ascent（15.08）远大于主字体的 `ceil(ascent)`，**差值更大**，所以汉字顶部裁得更明显（用户截图现象）。

## 3. 修复

### 3.1 baseline：line_gap 居中 + 下行优先（CPU/GPU 共用）

```rust
pub fn baseline_from_metrics(cell_h: u32, lm: &fontdue::LineMetrics) -> i32 {
    let ascent_px = lm.ascent.ceil().max(0.0);
    let gap_half = (lm.line_gap / 2.0).floor().max(0.0);
    let descent_px = (-lm.descent).ceil().max(0.0);
    let mut baseline = ascent_px + gap_half;          // line_gap 一半在上、一半在下
    if cell_h as f32 - baseline < descent_px {       // 下行放不下 → 先保下行
        baseline = cell_h as f32 - descent_px;
        if baseline < 0.0 { baseline = 0.0; }
    }
    baseline.clamp(0.0, cell_h as f32) as i32
}
```

效果：Consolas 9 → **11**（`ceil(9.65)=10 + floor(2.22/2)=1`），恰好等于 `ceil(ascent)+1`，高字形不再越顶；FreeMono 10 → 9（`cell_h - ceil(-descent) = 13-3`，下行优先）。

**取舍记录**（DECISIONS）：当 `ceil(ascent) + floor(line_gap/2)` 会让下行放不下时，**优先保下行**（`baseline = cell_h - ceil(-descent)`），顶部不足交由下面的逐字形平移兜底。理由：下行被裁比顶部被裁更影响可读性（g/p/y 的尾部），且顶部可由平移补偿。

### 3.2 逐字形垂直平移（CJK 回退字体兜底，不缩放）

```rust
pub fn glyph_shift(baseline: i32, ymin: i32, height: i32, cell_h: u32) -> i32 {
    let top = baseline - ymin - height;
    let mut shift = 0;
    if top < 0 { shift = -top; }                       // 顶越界 → 整体下移
    if height <= cell_h as i32 {
        let overflow = top + height + shift - cell_h as i32;
        if overflow > 0 { shift -= overflow; }         // 底越界 → 回移
    }
    shift
}
```

- 只做**整体平移**，不改任何缩放（宽字符的 2 格内缩放逻辑保持原样）。
- 字形高度 ≤ cell_h 时必然能完整落入格内。
- 字形高度 > cell_h（如 FreeMono cell_h=13 下某 CJK 字形高 14）：**保上顶、允许底部裁 1 行**（DECISIONS 记录取舍；实测只有 1 个字符如此）。
- CPU 的 `blit`/`blit_subpixel`、GPU 的普通字形/光标字符/Preedit 三处 `rect.y` 全部叠加同一 `shift`，两侧共用一个纯函数。

### 3.3 顺带修掉的 GPU 分层缺陷（已记 DECISIONS）

改了 Preedit 下划线所在层的顺序：GPU 的 Preedit 下划线原本与背景/样式线同层（在字形之前画），会被 Preedit 字形盖住（对拍实测 42 像素差、`all_max=250`）。给 `FrameInstances` 增加 `tail_n` 分区：quads 末尾的这部分在**所有字形之后**再画一次，与 CPU 的"底色 → 字形 → 下划线"顺序一致。

## 4. 滚动条加宽（用户要求）

- 宽度 4px → **8px**，统一为 `render::SCROLLBAR_WIDTH` 常量，三处引用：CPU 绘制（`draw_scrollbar`）、GPU 绘制（`build_instances` 的滚动条段）、app 命中测试（`hit_scrollbar`）。
- 同步修掉条带备份的硬编码：`capture_band` / `capture_band_char_row` / `restore_band` 原本按 4px 维护右缘条带，改宽后增量路径会与全量不一致（实测 `incremental_matches_full_pixel_exact` 在 x=352 出现 0xff7f7f7f vs 0xff000000）。
- case29 的取样点从右下角改为左下角（右侧 8px 现在是滚动条条带，不能当背景取样）。
- 理由：4px 在 HiDPI/4K 屏上难以命中（用户反馈"鼠标不容易点中"）。

## 5. 测试与门禁

新增 `crates/render/tests/glyph_clip.rs`（2 用例）：
1. `glyph_fits_in_cell_font_matrix`：字体矩阵（DejaVu/Liberation/Noto/FreeMono + `TEST_FONT_FILE` 外部字体 + CJK 回退），既有硬断言"顶部绝不裁"，也单独统计"字形高于格 → 保顶裁底"的数量并在输出中标注。
2. `glyph_top_not_clipped_gpu`：GPU 渲染 40 个汉字，断言"贴顶格数 = 0"；无适配器显式 `SKIP:` 并计入 run_all 的 `GPU_SKIP`。

**4B-fix1 的绝对基线测试全部仍绿**（`baseline_alignment` 2 用例：'.' 差 0–1px、g/p/y 下行、H 顶边正确；含 Consolas 外部字体）。

`gpu_parity` 8 用例：修复 baseline 后仍全过（两侧同改）；修 preedit 分层后恢复（此前 1 个用例失败）。

## 6. 验收

- `cargo test --workspace`：**119 passed / 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings`：**0**
- `cargo clippy --release --workspace --all-targets -- -D warnings`：**0**
- `cargo build --release`：**0 warning**
- `scripts/e2e/run_all.sh`：**87/87 PASS**，`GPU_SKIP=0`，GPU 轮 4/4

## 7. 未解决问题

1. **字形高度 > cell_h 时无法两全**（已验证事实）：FreeMono（cell_h=13）下 1 个 CJK 字形高 14，按"保上顶"允许底部裁 1 行。若改成保底则会裁顶（当前用户反馈的正是顶部裁剪，故不选）。推翻条件：若该字形是可读性关键字符且底部裁剪明显，需调大 `font.size` 或改用更高 cell 的字体。
2. **cell_h 由主字体决定**（工程判断，置信度中）：CJK 回退字体 ascent（15.08）大于主字体时，靠逐字形平移补偿而非扩大 cell_h。若主字体与回退字体行高差异极大（如 cell_h=13 + ascent=17），平移后字形会互相挤压（视觉上更密），但不裁剪。推翻条件：多字体混排出现明显行内高度不一致时，考虑按链内最大 ascent 计算 cell_h（会改变窗口尺寸语义，需单独立项）。
3. **滚动条加宽是行为变更**：8px 会占用约 1 个字符宽（默认 cell 8px），右侧一列内容被条带覆盖。若用户觉得遮挡过多，可改为"仅在滚动时显示"（需新配置项，未做）。
4. 真实 office-pc 上的 Consolas + 黑体验证仍需人工确认（见手动清单）。

## 8. 手动清单（≤3）

1. office-pc 上 `backend="cpu"`、Consolas 13、fallback 黑体、120×30：确认汉字与大写字形顶部完整（对比修复前截图），并确认 `|`、`{`、`[` 等最高字符不被裁。
2. 用鼠标点右侧滚动条拖动，确认 8px 宽度命中舒适；若觉得遮挡一列内容，反馈是否改为悬停/滚动时才显示。
3. 在 HiDPI（scale=2）环境下确认滚动条与字形的相对比例仍合适（本轮只在 :99 的 1x/2x 自动验证了像素一致性）。
