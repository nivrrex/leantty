#!/bin/bash
# V5：接口清单核对（可重复执行）。
# 对 11 项语义能力逐项检查 Rust 绑定 API 是否存在，并标注上游 C 头文件对应 API。
set -u
SRC=/root/Desktop/tty/vt-verify/vendor/libghostty-rs/crates/libghostty-vt/src
UP=/root/Desktop/tty/vt-verify/vendor/ghostty-src/include/ghostty/vt

check() { # <编号> <能力> <绑定符号> <绑定文件:行> <上游对应>
  local sym="$3" loc="$4" up="$5"
  if grep -rqn -- "$sym" "$SRC"; then
    printf '%-2s | %-40s | 有 | %-28s | %s | %s\n' "$1" "$2" "$sym" "$loc" "$up"
  else
    printf '%-2s | %-40s | 无 | %-28s | %s | %s\n' "$1" "$2" "$sym" "$loc" "$up"
  fi
}

echo "编号 | 能力 | 绑定 | API | 位置(相对 src/) | 上游 C API(vt.h)"
echo "---|---|---|---|---|---"
check 1 "遍历视口行与单元格" "RowIterator::new" render.rs "ghostty_render_state_row_iterator_new/next; ghostty_render_state_row_cells_new/next (render.h)"
check 2 "单元格字符与宽度/续格标记" "fn graphemes" render.rs "ghostty_grid_ref_graphemes (grid_ref.h); ghostty_cell_get (screen.h)"
check 2 "单元格宽度/续格" "fn wide" screen.rs "ghostty_cell_get wide 字段 (screen.h)"
check 3 "前景/背景色与样式" "fn fg_color" render.rs "ghostty_render_state_row_cells_get (render.h)"
check 3 "样式属性(bold/underline/inverse...)" "pub struct Style" style.rs "ghostty_style_default (style.h)"
check 4 "光标位置" "fn cursor_x" terminal.rs "ghostty_terminal_get TERMINAL_OPT_CURSOR (terminal.h)"
check 4 "光标可见性/样式" "fn cursor_visible" render.rs "ghostty_render_state_get CURSOR_* (render.h)"
check 5 "脏行/脏标记" "fn next_dirty" render.rs "ghostty_render_state_row_iterator_next_dirty (render.h)"
check 6 "滚动偏移" "fn viewport_row" render.rs "ghostty_render_state_get ROW_ITERATOR 相关 (render.h)"
check 6 "scrollback 总行数" "fn scrollback_rows" terminal.rs "ghostty_terminal_get TERMINAL_OPT_HISTORY_ROWS 类 (terminal.h)"
check 7 "滚动操作(按行/顶/底)" "fn scroll_viewport" terminal.rs "ghostty_terminal_scroll_viewport (terminal.h:3585)"
check 8 "resize" "fn resize" terminal.rs "ghostty_terminal_resize (terminal.h:3289)"
check 9 "alternate screen 状态" "fn active_screen" terminal.rs "ghostty_terminal_get SCREEN 类 (terminal.h)"
check 10 "选区/可复制文本提取" "fn format_selection_buf" selection.rs "ghostty_terminal_select_word/select_line (selection.h:771)"
check 11 "终端模式标志(mouse/paste/app-cursor)" "fn mode" terminal.rs "ghostty_terminal_get MODE + modes.h 常量 (terminal.h/modes.h)"

echo
echo "== 上游有、绑定未封装的代表性 API（按任务要求标注）=="
for sym in ghostty_terminal_vt_write_until ghostty_terminal_continuation_write ghostty_formatter_format_alloc ghostty_key_encoder_setopt_from_terminal ghostty_terminal_compress ghostty_terminal_point_from_grid_ref; do
  hit=$(grep -rl "$sym" "$SRC" 2>/dev/null | head -1)
  [ -z "$hit" ] && echo "  $sym : 绑定未封装（上游 vt.h 存在）" || echo "  $sym : 已封装 $(basename "$hit")"
done
