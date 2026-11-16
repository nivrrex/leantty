//! IME 合成处理（任务 5，纯函数可单测）。
//!
//! winit Ime 事件语义：Enabled→开始组合；Preedit(s, cursor)→组合中间态
//! （s 为空串=清除）；Commit(s)→组合完成提交最终文本；Disabled→IME 关闭。
//! 本模块只负责 Commit 文本归一化（写 PTY 前）；Preedit 仅存储供渲染层
//! 显示（本阶段不绘制组合下划线，登记限制）。

/// IME Commit 文本归一化：换行统一 \r（终端输入语义，同粘贴规范化）。
/// Preedit 不经本函数（中间态不写 PTY）；空串原样返回。
pub fn normalize_ime_commit(text: &str) -> String {
    text.replace("\r\n", "\r").replace('\n', "\r")
}

/// Preedit 显示串提取：组合串为空时返 None（清除信号）。
/// 光标范围 (begin, end) 为字节索引；越界 clamp 到串长（外部输入防御）。
pub fn preedit_display(
    preedit: &str,
    cursor: Option<(usize, usize)>,
) -> Option<(String, Option<(usize, usize)>)> {
    if preedit.is_empty() {
        return None;
    }
    let len = preedit.chars().count();
    // 各自 clamp 到 [0, len] 后保序（begin≤end；非法输入 swap 防御）。
    let cur = cursor.map(|(b, e)| {
        let (b, e) = (b.min(len), e.min(len));
        if b <= e { (b, e) } else { (e, b) }
    });
    Some((preedit.to_string(), cur))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ime_commit_newlines() {
        assert_eq!(normalize_ime_commit("a"), "a", "无换行原样");
        assert_eq!(normalize_ime_commit("a\nb"), "a\rb", "LF→CR");
        assert_eq!(
            normalize_ime_commit("a\r\nb"),
            "a\rb",
            "CRLF→CR（先去 CRLF 防双 CR）"
        );
        assert_eq!(normalize_ime_commit(""), "", "空串原样");
        assert_eq!(
            normalize_ime_commit("你\n好"),
            "你\rb".replace('b', "好"),
            "CJK 多行归一"
        );
    }

    #[test]
    fn preedit_display_empty_and_clamp() {
        assert_eq!(preedit_display("", None), None, "空串=清除信号");
        let (s, cur) = preedit_display("中", Some((0, 99))).expect("非空应有值");
        assert_eq!(s, "中");
        assert_eq!(cur, Some((0, 1)), "cursor end 越界 clamp 到字符数");
        let (_, cur) = preedit_display("中文", Some((5, 3))).expect("非空");
        assert_eq!(cur, Some((2, 2)), "begin/end 均 clamp 到 len 且保序");
    }

    #[test]
    fn preedit_display_none_cursor() {
        // None cursor=隐藏光标，保留串
        let (s, cur) = preedit_display("拼", None).expect("非空");
        assert_eq!(s, "拼");
        assert_eq!(cur, None, "None cursor 透传");
    }
}
