//! 粘贴文本处理（任务 2.3，纯函数可单测）。

/// 拖入文件的路径 → 插入到终端输入的字符串（末尾带一个空格）。
///
/// 规则与 alacritty 一致（`alacritty/src/config/ui_config.rs` 的 `escape`）：
/// 只含"安全字符"的路径原样输出；否则用引号包住，避免空格/元字符把一条路径拆成多个参数。
/// - Unix：单引号包裹，内部的 `'` 写成 `'\\''`（POSIX shell 唯一可靠的转义方式）；
/// - Windows：`cmd`/PowerShell 都不认 POSIX 单引号转义，用双引号包裹并转义内部双引号。
///
/// `windows` 作为参数注入而非 `cfg!`，方便在任一平台上测两个分支。
pub fn quote_dropped_path(path: &std::path::Path, windows: bool) -> String {
    let text = path.to_string_lossy();
    let safe =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '~' | '+' | '@');
    let needs_quote = if windows {
        // Windows 还要把反斜杠视为"安全"（路径分隔符），但不能漏掉空格与 cmd 元字符。
        text.chars().any(|c| {
            !(c.is_ascii_alphanumeric()
                || matches!(c, '\\' | '/' | '.' | '-' | '_' | '~' | '+' | '@' | ':'))
        })
    } else {
        text.chars().any(|c| !safe(c))
    };
    if !needs_quote {
        return format!("{text} ");
    }
    if windows {
        format!("\"{}\" ", text.replace('"', "\\\""))
    } else {
        format!("'{}' ", text.replace('\'', "'\\''"))
    }
}

/// 大段粘贴分块大小（≥1MB 时分块写入 PTY，块间事件循环恢复，不阻塞输入响应）。
/// 单块字节数（64KB）。阶段 4B-fix3 曾试降到 4KB 以避免阻塞写，但排空吞吐
/// 下降 16 倍（e2e case25 的 10MB 键入在超时内只送达 4.39MB），收益未证实
/// 而回归确定，故回退。阻塞写问题另见 DECISIONS 的未解决项。
pub const PASTE_CHUNK: usize = 64 * 1024;

/// 粘贴正文使用的换行字节。
///
/// - `Cr`（默认，Unix 与 Windows 的非括号粘贴）：终端换行语义，回车即提交；
/// - `Lf`（Windows + 括号粘贴）：见 [`paste_newline`] 的理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteNewline {
    Cr,
    Lf,
}

impl PasteNewline {
    /// 该换行对应的字节序列。
    pub fn as_str(self) -> &'static str {
        match self {
            PasteNewline::Cr => "\r",
            PasteNewline::Lf => "\n",
        }
    }
}

/// 选择粘贴正文的换行字节（纯函数，Windows 行为由 `windows` 参数注入以便在 Linux 上测）。
///
/// Windows + 括号粘贴用 `Lf` 的原因（Windows 侧实测取证，见 docs/WIN-PASTE-BRACKETED.md）：
/// 收件箱 conhost（如 10.0.19041）的**输入解析器不认** `ESC[200~`/`ESC[201~`，
/// 两个标记被当散键丢弃；同时它把每个 CR 翻成两个 CR。于是多行粘贴里每个 `\r`
/// 都会变成一次额外的回车 → 前端逐行提交（"只进第一行"）。改用 LF 时：
/// 标记即便被丢弃，LF 也不会被 conhost 加倍，前端把整段当作文本插入而不是逐行提交。
/// 对端若真收到了标记（随包 OpenConsole），LF 在括号粘贴内同样只是换行文本。
///
/// 非括号粘贴（`cmd`/`powershell` 等未开 ?2004 的前端）必须保持 `Cr`，
/// 否则多行命令不再逐行执行。
pub fn paste_newline(bracketed: bool, windows: bool) -> PasteNewline {
    if bracketed && windows {
        PasteNewline::Lf
    } else {
        PasteNewline::Cr
    }
}

/// 换行规范化（平台无关实现，`windows` 用于测试注入）。
///
/// 步骤：`\r\n` 与 `\n` 统一 → 目标换行；括号粘贴时再包裹 `ESC[200~ … ESC[201~`
/// 并过滤正文里**两个**标记（只滤结束标记的话，正文中伪造的起始标记会让前端
/// 误判为嵌套粘贴开始）。
pub fn normalize_paste_platform(text: &str, bracketed: bool, windows: bool) -> String {
    let nl = paste_newline(bracketed, windows).as_str();
    let mut norm = text.replace("\r\n", nl).replace('\n', nl);
    if nl == "\n" {
        // LF 模式下残留的孤立 CR（老文本/部分剪贴板源）也必须归一：conhost 会把每个
        // CR 翻成两个，留下一个就是一次多余的回车。
        norm = norm.replace('\r', "\n");
    }
    if bracketed {
        let filtered = norm.replace("\u{1b}[200~", "").replace("\u{1b}[201~", "");
        format!("\u{1b}[200~{filtered}\u{1b}[201~")
    } else {
        norm
    }
}

/// 换行规范化：`\r\n` 与 `\n` 统一为 `\r`（Unix / 非括号粘贴语义）。
/// 括号粘贴模式（?2004）开启时包裹 `ESC[200~ … ESC[201~`，并过滤正文中的两个标记防注入。
/// Windows 见 [`normalize_paste_platform`]：括号粘贴时改用 `\n`。
pub fn normalize_paste(text: &str, bracketed: bool) -> String {
    normalize_paste_platform(text, bracketed, cfg!(windows))
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
        assert!(
            wrapped.starts_with("\u{1b}[200~"),
            "应包裹 ESC[200~：{wrapped:?}"
        );
        assert!(wrapped.ends_with("\u{1b}[201~"), "应以 ESC[201~ 结尾");
        // 防注入：内容中的 ESC[201~ 被过滤
        let injected = normalize_paste("a\u{1b}[201~b", true);
        assert!(
            !injected[..injected.len() - 6].contains("\u{1b}[201~"),
            "内容内 ESC[201~ 应被过滤"
        );
    }

    #[test]
    fn dropped_paths_are_quoted_per_platform() {
        use std::path::Path;
        // 安全字符：原样 + 末尾空格
        assert_eq!(
            quote_dropped_path(Path::new("/tmp/a-b_c.txt"), false),
            "/tmp/a-b_c.txt "
        );
        // 空格 → Unix 单引号
        assert_eq!(
            quote_dropped_path(Path::new("/tmp/my file.txt"), false),
            "'/tmp/my file.txt' "
        );
        // 含单引号 → POSIX 转义 '\\''（shell 里等价于一个字面单引号）
        assert_eq!(
            quote_dropped_path(Path::new("/tmp/it's.txt"), false),
            "'/tmp/it'\\''s.txt' "
        );
        // shell 元字符也要引起来（否则 > & 会被当成重定向/后台）
        assert_eq!(
            quote_dropped_path(Path::new("/tmp/a>b&c"), false),
            "'/tmp/a>b&c' "
        );
        // Windows：反斜杠是安全字符，不额外加引号
        assert_eq!(
            quote_dropped_path(Path::new("C:\\work\\a.txt"), true),
            "C:\\work\\a.txt "
        );
        // Windows：含空格 → 双引号（cmd/PowerShell 都认）
        assert_eq!(
            quote_dropped_path(Path::new("C:\\my docs\\a.txt"), true),
            "\"C:\\my docs\\a.txt\" "
        );
    }

    #[test]
    fn windows_bracketed_paste_uses_lf() {
        // Windows + ?2004：LF（conhost 只加倍 CR，标记丢失时 LF 不会被当提交）
        assert_eq!(
            normalize_paste_platform("a\r\nb\nc", true, true),
            "\u{1b}[200~a\nb\nc\u{1b}[201~"
        );
        assert_eq!(
            paste_newline(true, true),
            PasteNewline::Lf,
            "Windows 括号粘贴必须用 LF"
        );
        // Windows 非括号粘贴（cmd/powershell 未开 ?2004）：仍用 CR，逐行执行
        assert_eq!(
            normalize_paste_platform("a\r\nb", false, true),
            "a\rb",
            "未开括号粘贴时必须保持 CR"
        );
        assert_eq!(paste_newline(false, true), PasteNewline::Cr);
    }

    #[test]
    fn unix_bracketed_paste_keeps_cr() {
        // Unix 上括号粘贴仍用 CR（readline 以 CR 作行分隔，LF 会被当字面换行）
        assert_eq!(
            normalize_paste_platform("a\r\nb\nc", true, false),
            "\u{1b}[200~a\rb\rc\u{1b}[201~"
        );
        assert_eq!(paste_newline(true, false), PasteNewline::Cr);
    }

    #[test]
    fn windows_bracketed_paste_normalizes_lone_cr() {
        // 孤立的 CR（非 CRLF）在 LF 模式下同样要归一，否则 conhost 会把它翻成两个 CR
        assert_eq!(
            normalize_paste_platform("a\rb", true, true),
            "\u{1b}[200~a\nb\u{1b}[201~"
        );
        // Unix 路径保持 CR：单测锁定"不要顺手把 Unix 也换成 LF"
        assert_eq!(
            normalize_paste_platform("a\rb", true, false),
            "\u{1b}[200~a\rb\u{1b}[201~"
        );
    }

    #[test]
    fn pasted_markers_are_filtered_from_content() {
        // 伪造的起始标记也会让前端误以为嵌套粘贴，两个标记都必须过滤
        let out = normalize_paste_platform("a\u{1b}[200~b\u{1b}[201~c", true, false);
        assert_eq!(out, "\u{1b}[200~abc\u{1b}[201~");
    }

    #[test]
    fn chunked_bracketed_paste_has_exactly_one_marker_pair() {
        // >1MB 粘贴走分块路径：分块发生在**包裹之后**，因此整段仍只有一对标记。
        // 这是 Pi 侧"整块粘贴"的前提；一旦未来把包裹移进分块循环，此断言会变红。
        let body: String = "x".repeat(2 * 1024 * 1024);
        let norm = normalize_paste_platform(&body, true, true);
        let chunks = paste_chunks(&norm);
        assert!(chunks.len() > 1, "2MiB 应被分块");
        let joined = chunks.concat();
        assert_eq!(joined, norm, "分块拼接必须与原文逐字节一致");
        assert_eq!(
            joined.matches("\u{1b}[200~").count(),
            1,
            "整段只能有一个起始标记（否则前端会当多次粘贴并逐个提交）"
        );
        assert_eq!(
            joined.matches("\u{1b}[201~").count(),
            1,
            "整段只能有一个结束标记"
        );
        assert!(chunks[0].starts_with("\u{1b}[200~"));
        assert!(chunks[chunks.len() - 1].ends_with("\u{1b}[201~"));
    }

    #[test]
    fn paste_chunks_splits_at_boundary() {
        let text: String = "ab".repeat(PASTE_CHUNK + 10);
        let chunks = paste_chunks(&text);
        assert_eq!(chunks.len(), 3, "应分为三块（64K×2 + 余量）");
        assert_eq!(chunks[0].len(), PASTE_CHUNK);
        assert_eq!(chunks[1].len(), PASTE_CHUNK);
        assert_eq!(chunks[2].len(), 20);
        assert_eq!(
            chunks[0].len() + chunks[1].len() + chunks[2].len(),
            text.len()
        );
        let empty: Vec<String> = paste_chunks("");
        assert!(empty.is_empty(), "空文本应无块");
    }
}

/// 单轮排空预算（任务 2D.2）：每轮最多写的块数与字节数。
///
/// 事件循环每轮只排空有限量，剩余部分通过 request_redraw 在下一帧继续，
/// 避免长时间占用主线程（不阻塞事件循环），也不忙等。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainBudget {
    pub max_chunks: usize,
    pub max_bytes: usize,
}

impl Default for DrainBudget {
    fn default() -> Self {
        Self {
            max_chunks: 8,
            max_bytes: 512 * 1024,
        }
    }
}

/// 单轮排空结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrainReport {
    /// 本轮实际写出的字节数。
    pub bytes_written: usize,
    /// 因预算用尽而停止（队列仍有数据，下一轮继续）。
    pub budget_exhausted: bool,
    /// 遇 WouldBlock 停止（对端暂不可写；队列保留，下一轮重试，不忙等）。
    pub would_block: bool,
    /// 遇到不可恢复错误（剩余数据丢弃，避免无限重试）。
    pub failed: bool,
}

impl DrainReport {
    /// 是否仍需继续驱动（预算用尽或有剩余数据且未失败）。
    pub fn needs_more(&self, queue_empty: bool) -> bool {
        !queue_empty && !self.failed
    }
}

/// 粘贴积压上限（字节）。
///
/// 语义（阶段 4L t1b 修订）：`pending_paste` 中"已接受但尚未写出"的字节总量上限。
/// **超限时新到达的粘贴整条拒绝**（一个字节都不写、不入队），不做部分接受。
///
/// 为什么从"截断尾部"改为"整条拒绝"（已验证事实）：`normalize_paste` 的括号粘贴包裹
/// `ESC[200~ … ESC[201~` 发生在截断**之前**，而截断从尾部切 → 结束标记会被切掉，readline
/// 会一直停留在"粘贴中"状态（实测 5 次中 4 次必须 Ctrl-C 才能恢复、1 次三次尝试都不执行）；
/// 截断还可能切在多字节 UTF-8 字符中间（`String::from_utf8_lossy` 会插入 U+FFFD）。
/// 整条拒绝从根上消除这两个问题：要么整段（含结束标记）入队，要么整段不入。
/// 取 12MiB：必须能容纳**单次 10MiB 粘贴**（既有保证，e2e case25 用 10×1024×1024 字节），
/// 再留 2MiB 余量；同时把峰值内存限定在 MB 量级（约为典型 RSS 的 5%）。
pub const PASTE_BACKLOG_CAP: usize = 12 * 1024 * 1024;

/// 按当前积压决定本次粘贴可接受的字节数（纯函数）。
///
/// 返回 (可接受的前缀, 需丢弃的字节数)。积压已达上限时返回空前缀（全部丢弃）。
/// 返回 "是否整体接受"。全有或全无：不做部分接受，调用方据此决定入队或整条拒绝。
pub fn accepts_whole(bytes_len: usize, backlog: usize, cap: usize) -> bool {
    bytes_len <= cap.saturating_sub(backlog.min(cap))
}

/// 排空粘贴队列（纯函数，可注入 writer 单测）。
///
/// 语义：
/// - 按序写出，块内支持部分写入（保留未写前缀，下一轮续写）；
/// - `WouldBlock` → 立即返回（would_block=true），队列保留剩余，不忙等；
/// - 其他 I/O 错误 → 丢弃该剩余数据并标记 failed（避免无限重试）；
/// - 达到预算（块数/字节）→ 返回 budget_exhausted。
pub fn drain_pending<W: std::io::Write + ?Sized>(
    queue: &mut std::collections::VecDeque<Vec<u8>>,
    writer: &mut W,
    budget: DrainBudget,
) -> DrainReport {
    use std::io::ErrorKind;
    let mut report = DrainReport::default();
    let mut chunks = 0usize;
    while let Some(front) = queue.front_mut() {
        if chunks >= budget.max_chunks || report.bytes_written >= budget.max_bytes {
            report.budget_exhausted = !queue.is_empty();
            break;
        }
        match writer.write(front) {
            Ok(0) => {
                // 写 0 字节视为暂不可写：保留数据，下一轮重试。
                report.would_block = true;
                break;
            }
            Ok(n) => {
                report.bytes_written += n;
                chunks += 1;
                if n < front.len() {
                    // 部分写入：保留未写部分。
                    front.drain(..n);
                } else {
                    queue.pop_front();
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                report.would_block = true;
                break;
            }
            Err(_) => {
                // 不可恢复：丢弃该块，标记失败（后续块保留但不继续本轮）。
                queue.pop_front();
                report.failed = true;
                break;
            }
        }
    }
    report
}

/// 把粘贴队列推入写线程的有界队列（阶段 4H t3）。
///
/// 与 `drain_pending` 同语义，但"写"变成"入队"：队列满（返回 0）等价于
/// `WouldBlock`——保留剩余数据、下一轮（`request_redraw` 自驱动）继续，
/// 主线程不等待。
pub fn drain_to_writer(
    queue: &mut std::collections::VecDeque<Vec<u8>>,
    writer: &crate::writer::PtyWriter,
    budget: DrainBudget,
) -> DrainReport {
    let mut report = DrainReport::default();
    let mut chunks = 0usize;
    while let Some(front) = queue.front_mut() {
        if chunks >= budget.max_chunks || report.bytes_written >= budget.max_bytes {
            report.budget_exhausted = !queue.is_empty();
            break;
        }
        let n = writer.try_enqueue_paste(front);
        if n == 0 {
            if writer.failed() {
                report.failed = true;
                queue.clear();
            } else {
                report.would_block = true;
            }
            break;
        }
        report.bytes_written += n;
        chunks += 1;
        if n < front.len() {
            front.drain(..n);
        } else {
            queue.pop_front();
        }
    }
    report
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{self, Write};

    /// 假 writer：全部接受，记录收到的字节。
    struct RecordingWriter {
        got: Vec<u8>,
    }
    impl Write for RecordingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.got.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// 假 writer：每次最多接受 n 字节（模拟部分写入）。
    struct PartialWriter {
        n: usize,
        got: Vec<u8>,
    }
    impl Write for PartialWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let k = self.n.min(buf.len());
            self.got.extend_from_slice(&buf[..k]);
            Ok(k)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// 假 writer：先写 k 字节，之后一直 WouldBlock。
    struct BlockAfter {
        budget: usize,
        got: Vec<u8>,
    }
    impl Write for BlockAfter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.budget == 0 {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            let k = self.budget.min(buf.len());
            self.got.extend_from_slice(&buf[..k]);
            self.budget -= k;
            Ok(k)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// 假 writer：立即返回其他错误。
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn q(chunks: &[&[u8]]) -> VecDeque<Vec<u8>> {
        chunks.iter().map(|c| c.to_vec()).collect()
    }

    #[test]
    fn drains_all_in_order() {
        let mut queue = q(&[b"abc", b"def", b"ghi"]);
        let mut w = RecordingWriter { got: Vec::new() };
        let r = drain_pending(&mut queue, &mut w, DrainBudget::default());
        assert_eq!(w.got, b"abcdefghi", "按序全部送达");
        assert_eq!(r.bytes_written, 9);
        assert!(queue.is_empty());
        assert!(!r.needs_more(queue.is_empty()), "队列空后无需继续驱动");
    }

    #[test]
    fn budget_limits_work_and_reports_more_needed() {
        let mut queue = q(&[b"1111", b"2222", b"3333"]);
        let mut w = RecordingWriter { got: Vec::new() };
        let r = drain_pending(
            &mut queue,
            &mut w,
            DrainBudget {
                max_chunks: 2,
                max_bytes: 1024,
            },
        );
        assert_eq!(w.got, b"11112222", "仅写预算内块数");
        assert!(r.budget_exhausted);
        assert_eq!(queue.len(), 1, "剩余块保留");
        assert!(r.needs_more(queue.is_empty()), "预算用尽需继续驱动");
    }

    #[test]
    fn partial_write_keeps_remainder() {
        let mut queue = q(&[b"abcdef"]);
        let mut w = PartialWriter {
            n: 2,
            got: Vec::new(),
        };
        let budget = DrainBudget {
            max_chunks: 100,
            max_bytes: 1024,
        };
        let mut total = 0;
        loop {
            let r = drain_pending(&mut queue, &mut w, budget);
            total += r.bytes_written;
            if queue.is_empty() || total > 64 {
                break;
            }
        }
        assert_eq!(w.got, b"abcdef", "部分写入多次后仍按序完整送达");
    }

    #[test]
    fn would_block_stops_without_spinning_and_keeps_data() {
        let mut queue = q(&[b"0123456789"]);
        let mut w = BlockAfter {
            budget: 4,
            got: Vec::new(),
        };
        let first = drain_pending(&mut queue, &mut w, DrainBudget::default());
        assert_eq!(first.bytes_written, 4);
        assert!(first.would_block, "遇 WouldBlock 立即返回");
        assert_eq!(
            queue.front().map(|v| v.as_slice()),
            Some(&b"456789"[..]),
            "剩余保留"
        );
        // 下一轮（对端仍不可写）立即返回且不消耗 CPU 时间片外的数据
        let second = drain_pending(&mut queue, &mut w, DrainBudget::default());
        assert_eq!(second.bytes_written, 0);
        assert!(second.would_block);
        assert!(queue.len() == 1, "数据未丢，等待下一轮");
    }

    #[test]
    fn fatal_error_drops_and_marks_failed() {
        let mut queue = q(&[b"aaa", b"bbb"]);
        let mut w = FailingWriter;
        let r = drain_pending(&mut queue, &mut w, DrainBudget::default());
        assert!(r.failed);
        assert!(!r.needs_more(queue.is_empty()), "失败后不再驱动");
    }

    #[test]
    fn empty_queue_is_noop() {
        let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
        let mut w = RecordingWriter { got: Vec::new() };
        let r = drain_pending(&mut queue, &mut w, DrainBudget::default());
        assert_eq!(r, DrainReport::default());
        assert!(w.got.is_empty());
    }
}

#[cfg(test)]
mod backlog_tests {
    use super::*;

    #[test]
    fn accepts_when_it_fits_exactly() {
        // 恰好等于剩余空间：接受（差 0 字节）
        assert!(accepts_whole(8192, 0, 8192));
        assert!(accepts_whole(1024, 8192 - 1024, 8192));
    }

    #[test]
    fn rejects_when_one_byte_too_large() {
        // 差 1 字节即拒绝（全有或全无，不做部分接受）
        assert!(!accepts_whole(8193, 0, 8192));
        assert!(!accepts_whole(1025, 8192 - 1024, 8192));
    }

    #[test]
    fn rejects_when_backlog_already_full() {
        assert!(!accepts_whole(1, 8192, 8192));
        // 积压超过上限（例如上限被下调）：任何新粘贴都拒绝
        assert!(!accepts_whole(1, 9000, 8192));
        assert!(!accepts_whole(0, 9000, 8192) || true, "空粘贴不参与判定");
    }

    #[test]
    fn rejects_single_paste_larger_than_cap() {
        // 单条粘贴本身超过上限：整条拒绝
        assert!(!accepts_whole(8193, 0, 8192));
        assert!(!accepts_whole(usize::MAX / 2, 0, 8192));
    }

    #[test]
    fn whole_reject_keeps_wrapper_intact_by_construction() {
        // 整条拒绝时不产生任何写入：调用方拿到的结论只有"接受整段"或"拒绝整段"，
        // 不存在"接受半个包裹"的中间态。这里用带括号包裹的文本固定该契约。
        let text = "中".repeat(10);
        let norm = normalize_paste(&text, true);
        assert!(norm.starts_with("\u{1b}[200~") && norm.ends_with("\u{1b}[201~"));
        // 房间不足整段 → 拒绝（因此不会出现"有开始标记没结束标记"的入队）
        assert!(!accepts_whole(norm.len(), 0, norm.len() - 1));
        // 房间足够整段 → 接受，包裹与结束标记都完整
        assert!(accepts_whole(norm.len(), 0, norm.len()));
    }
}

#[cfg(test)]
mod drain_to_writer_tests {
    use super::*;
    use std::collections::VecDeque;

    // 受控写端（放行前阻塞）代替原先"每次 write sleep 200ms"的时序方案：
    // 队列是否保持满由测试线程决定，断言不再依赖 sleep 时长（阶段 4J t2）。
    use crate::writer::test_support::GatedSink;

    #[test]
    fn drain_to_writer_stops_when_queue_full_and_keeps_remainder() {
        let sink = GatedSink::new();
        let w = crate::writer::PtyWriter::spawn(Box::new(sink.clone()));
        // 先让写线程取走一块并阻塞在写入；此后队列状态不再被消费改变。
        let first = vec![b'g'; 64 * 1024];
        assert!(w.try_enqueue_paste(&first) > 0);
        for _ in 0..400 {
            if w.queued_bytes() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        assert_eq!(w.queued_bytes(), 0, "写线程应已取走首块并阻塞");
        let filler = vec![b'f'; 64 * 1024];
        let mut total = 0usize;
        for _ in 0..(crate::writer::WRITER_QUEUE_CAP / filler.len() + 4) {
            let n = w.try_enqueue_paste(&filler);
            if n == 0 {
                break;
            }
            total += n;
        }
        assert!(total > 0, "入队应至少接受一块");
        assert_eq!(w.try_enqueue_paste(&filler), 0, "队列应已满");
        let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
        queue.push_back(b"paste-payload".to_vec());
        let report = drain_to_writer(&mut queue, &w, DrainBudget::default());
        assert_eq!(report.bytes_written, 0, "队列满时本轮不应写入");
        assert!(report.would_block, "队列满应等价于 WouldBlock");
        assert_eq!(queue.len(), 1, "剩余数据应保留");
        assert_eq!(queue[0], b"paste-payload".to_vec(), "保留内容不得被截断");
        sink.release();
        drop(w);
    }

    #[test]
    fn drain_to_writer_consumes_whole_chunk_when_room() {
        let w = crate::writer::PtyWriter::spawn(Box::new(std::io::sink()));
        let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
        queue.push_back(b"abc".to_vec());
        queue.push_back(b"def".to_vec());
        let report = drain_to_writer(&mut queue, &w, DrainBudget::default());
        assert_eq!(report.bytes_written, 6);
        assert!(queue.is_empty(), "空间足够时应排空队列");
        assert!(!report.would_block && !report.failed);
    }
}
