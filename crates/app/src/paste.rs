//! 粘贴文本处理（任务 2.3，纯函数可单测）。

/// 大段粘贴分块大小（≥1MB 时分块写入 PTY，块间事件循环恢复，不阻塞输入响应）。
/// 单块字节数（64KB）。阶段 4B-fix3 曾试降到 4KB 以避免阻塞写，但排空吞吐
/// 下降 16 倍（e2e case25 的 10MB 键入在超时内只送达 4.39MB），收益未证实
/// 而回归确定，故回退。阻塞写问题另见 DECISIONS 的未解决项。
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
        let mut w = PartialWriter { n: 2, got: Vec::new() };
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
        assert_eq!(queue.front().map(|v| v.as_slice()), Some(&b"456789"[..]), "剩余保留");
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
mod drain_to_writer_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::Write;

    /// 极慢写端：让队列保持在满的状态，用于验证"队列满即停"。
    struct SlowSink;
    impl Write for SlowSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn drain_to_writer_stops_when_queue_full_and_keeps_remainder() {
        let w = crate::writer::PtyWriter::spawn(Box::new(SlowSink));
        // 塞满队列
        let filler = vec![b'f'; crate::writer::WRITER_QUEUE_CAP];
        assert_eq!(w.try_enqueue_paste(&filler), crate::writer::WRITER_QUEUE_CAP);
        let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
        queue.push_back(b"paste-payload".to_vec());
        let report = drain_to_writer(&mut queue, &w, DrainBudget::default());
        assert_eq!(report.bytes_written, 0, "队列满时本轮不应写入");
        assert!(report.would_block, "队列满应等价于 WouldBlock");
        assert_eq!(queue.len(), 1, "剩余数据应保留");
        assert_eq!(queue[0], b"paste-payload".to_vec(), "保留内容不得被截断");
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
