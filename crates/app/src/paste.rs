//! 粘贴文本处理（任务 2.3，纯函数可单测）。

/// 大段粘贴分块大小（≥1MB 时分块写入 PTY，块间事件循环恢复，不阻塞输入响应）。
pub const PASTE_CHUNK: usize = 64 * 1024;

/// 换行规范化：\r\n 与 \n 统一为 \r（终端换行语义）。
/// 括号粘贴模式（?2004）开启时包裹 ESC[200~ … ESC[201~，
/// 并过滤内容中的 ESC[201~ 防注入。
pub fn normalize_paste(text: &str, bracketed: bool) -> String {
    let norm = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let filtered = norm.replace("\u{1b}[201~", "");
        format!("\u{1b}[200~{filtered}\u{1b}[201~")
    } else {
        norm
    }
}

/// 大段粘贴按 PASTE_CHUNK 字符分块（UTF-8 边界安全，字符级切分）。
pub fn paste_chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if cur.len() >= PASTE_CHUNK {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_paste_newlines_and_bracketed() {
        assert_eq!(normalize_paste("a\nb", false), "a\rb");
        assert_eq!(normalize_paste("a\r\nb", false), "a\rb");
        assert_eq!(normalize_paste("a\rb", false), "a\rb");
        let wrapped = normalize_paste("x", true);
        assert!(wrapped.starts_with("\u{1b}[200~"), "应包裹 ESC[200~：{wrapped:?}");
        assert!(wrapped.ends_with("\u{1b}[201~"), "应以 ESC[201~ 结尾");
        // 防注入：内容中的 ESC[201~ 被过滤
        let injected = normalize_paste("a\u{1b}[201~b", true);
        assert!(!injected[..injected.len() - 6].contains("\u{1b}[201~"), "内容内 ESC[201~ 应被过滤");
    }

    #[test]
    fn paste_chunks_splits_at_boundary() {
        let text: String = "ab".repeat(PASTE_CHUNK + 10);
        let chunks = paste_chunks(&text);
        assert_eq!(chunks.len(), 3, "应分为三块（64K×2 + 余量）");
        assert_eq!(chunks[0].len(), PASTE_CHUNK);
        assert_eq!(chunks[1].len(), PASTE_CHUNK);
        assert_eq!(chunks[2].len(), 20);
        assert_eq!(chunks[0].len() + chunks[1].len() + chunks[2].len(), text.len());
        let empty: Vec<String> = paste_chunks("");
        assert!(empty.is_empty(), "空文本应无块");
    }
}
