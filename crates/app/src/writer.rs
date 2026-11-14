//! PTY 独立写线程（阶段 4H t3）。
//!
//! ## 背景（已验证事实）
//! 前台程序不读 stdin 时（如 `sleep`、忙编译），PTY 输入队列被占满后
//! `write_all` 会在**主线程**阻塞：release 实测 1MB 粘贴在规范模式下造成
//! **12.9s** 主线程停顿（`loop_gap_ms=12936`），粘贴期间界面完全不响应。
//!
//! ## 契约
//! - **顺序**：键入与粘贴共用一条队列，单消费者线程按入队顺序写出。
//! - **有界**：`WRITER_QUEUE_CAP`（1MiB）只约束**粘贴**入队；键入量小且不得
//!   丢失，因此键入始终入队（可短暂超过上限，随后被消费）。
//! - **Ctrl-C（0x03）**：入队 0x03 前先丢弃队列中**尚未写出**的粘贴数据，
//!   使用户中断立即生效（行为决策，见 DECISIONS）。
//! - **关闭**：`close()` 置 closing 并唤醒线程，线程写出队列剩余后退出并
//!   drop 写端（= 释放 PTY 主端）；join 有超时，超时则分离线程并记录日志，
//!   调用方随后才 `Pty::close()` 回收子进程。
//! - **错误**：写端返回 EIO/EPIPE 等错误时不 panic：置 `failed` 状态、清空
//!   队列并退出线程；app 走既有的"子进程退出/关闭窗口"路径。
//!
//! ## 并发与所有权
//! 写端（`Box<dyn Write + Send>`）由写线程独占持有；主线程只通过
//! `PtyWriter` 的共享状态（`Mutex<State>` + `Condvar`）入队，不存在跨线程
//! 共享的可变写端。`PtyWriter` 非 `Sync` 之外的额外约束：可在单线程内自由
//! 使用，入队方法取 `&self`。

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};

/// 写队列字节上限（仅约束粘贴入队）。
///
/// 取值理由：`paste::PASTE_CHUNK` = 64KiB、单轮排空预算 `max_bytes` = 512KiB，
/// 1MiB 允许约两轮预算在途，既能让写线程始终有活干（不在主线程等待），
/// 又把队列内存限制在 1MiB（10MB 粘贴不会整体堆在主线程内存里）。
pub const WRITER_QUEUE_CAP: usize = 1 << 20;

/// 关闭时等待写线程退出的上限。
///
/// 取 2s：正常路径（队列已排空或对端可写）远小于该值；超时说明对端长期
/// 不读，此时必须放弃等待，否则应用退出会被 PTY 拖死。
pub const WRITER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// 队列元素。
struct Item {
    bytes: Vec<u8>,
    /// 来自粘贴（Ctrl-C 时丢弃）。
    paste: bool,
}

/// 共享状态（唯一可变状态，受互斥量保护）。
struct State {
    items: VecDeque<Item>,
    /// 队列中待写字节数（与 `items` 同步维护）。
    queued: usize,
    /// 关闭中：写线程在**写空队列后**退出。
    closing: bool,
    /// 写端错误（EIO/EPIPE 等），非 None 后不再接受入队。
    failed: Option<std::io::ErrorKind>,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // 互斥量中毒（写线程 panic）不应连带主线程 panic：取回内部状态继续。
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// PTY 写线程句柄。
pub struct PtyWriter {
    shared: Arc<Shared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PtyWriter {
    /// 以给定写端起写线程（写端由线程独占）。
    pub fn spawn(writer: Box<dyn Write + Send>) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                items: VecDeque::new(),
                queued: 0,
                closing: false,
                failed: None,
            }),
            cv: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("pty-writer".to_string())
            .spawn(move || worker(writer, worker_shared))
            .ok();
        Self { shared, handle }
    }

    /// 入队键入字节（不使用 `WRITER_QUEUE_CAP` 限制，保证不丢）。
    ///
    /// 含 0x03（Ctrl-C）时先丢弃尚未写出的粘贴数据，使中断立即生效。
    pub fn enqueue_typed(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut st = self.shared.lock();
        if st.failed.is_some() || st.closing {
            return;
        }
        if bytes.contains(&0x03) {
            st.items.retain(|it| !it.paste);
            st.queued = st.items.iter().map(|it| it.bytes.len()).sum();
        }
        st.queued += bytes.len();
        st.items.push_back(Item { bytes: bytes.to_vec(), paste: false });
        self.shared.cv.notify_one();
    }

    /// 尝试入队粘贴字节，返回实际接受的字节数（0 = 队列已满或已失败）。
    pub fn try_enqueue_paste(&self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        let mut st = self.shared.lock();
        if st.failed.is_some() || st.closing || st.queued >= WRITER_QUEUE_CAP {
            return 0;
        }
        let n = (WRITER_QUEUE_CAP - st.queued).min(bytes.len());
        st.queued += n;
        st.items.push_back(Item { bytes: bytes[..n].to_vec(), paste: true });
        self.shared.cv.notify_one();
        n
    }

    /// 写端是否已失败（EIO/EPIPE 等）。调用方可据此走既有退出路径。
    pub fn failed(&self) -> bool {
        self.shared.lock().failed.is_some()
    }

    /// 当前排队字节数（观测用）。
    pub fn queued_bytes(&self) -> usize {
        self.shared.lock().queued
    }

    /// 关闭：让线程写空队列后退出并释放写端；join 超时则分离并记录日志。
    pub fn close(&mut self) {
        {
            let mut st = self.shared.lock();
            st.closing = true;
        }
        self.shared.cv.notify_all();
        if let Some(handle) = self.handle.take() {
            // 经 channel 实现带超时 join（JoinHandle 本身无超时 join）。
            let (tx, rx) = std::sync::mpsc::channel();
            let joiner = std::thread::Builder::new()
                .name("pty-writer-join".to_string())
                .spawn(move || {
                    let _ = handle.join();
                    let _ = tx.send(());
                });
            match joiner {
                Ok(_) => {
                    if rx.recv_timeout(WRITER_JOIN_TIMEOUT).is_err() {
                        eprintln!(
                            "pty writer: join 超时（{}s），已分离线程（对端长期不读）",
                            WRITER_JOIN_TIMEOUT.as_secs()
                        );
                    }
                }
                Err(e) => eprintln!("pty writer: join 线程创建失败：{e}（已分离）"),
            }
        }
    }
}

impl Drop for PtyWriter {
    fn drop(&mut self) {
        self.close();
    }
}

/// 写线程主体：按序取出元素写出；队列空且 closing → 退出（drop writer 释放写端）。
fn worker(mut writer: Box<dyn Write + Send>, shared: Arc<Shared>) {
    loop {
        let item = {
            let mut st = shared.lock();
            loop {
                if let Some(item) = st.items.pop_front() {
                    st.queued -= item.bytes.len();
                    break Some(item);
                }
                if st.closing || st.failed.is_some() {
                    break None;
                }
                st = shared.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        let Some(item) = item else { return };
        if let Err(e) = writer.write_all(&item.bytes).and_then(|()| writer.flush()) {
            let mut st = shared.lock();
            st.failed = Some(e.kind());
            st.items.clear();
            st.queued = 0;
            shared.cv.notify_all();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::sync::Mutex as StdMutex;

    /// 记录写入内容的测试写端；可注入错误。
    #[derive(Clone, Default)]
    struct Sink {
        got: Arc<StdMutex<Vec<u8>>>,
        fail_kind: Option<ErrorKind>,
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(k) = self.fail_kind {
                return Err(std::io::Error::new(k, "注入错误"));
            }
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn wait_bytes(sink: &Sink, want: usize) {
        for _ in 0..200 {
            if sink.got.lock().unwrap().len() >= want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn paste_queue_respects_cap() {
        let sink = Sink::default();
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        // 先占满队列（写线程会并发消费，故循环入队直到拒绝）
        let chunk = vec![b'x'; 64 * 1024];
        let mut accepted = 0usize;
        let mut rejected = false;
        for _ in 0..64 {
            match w.try_enqueue_paste(&chunk) {
                0 => {
                    rejected = true;
                    break;
                }
                n => accepted += n,
            }
        }
        assert!(rejected, "持续入队应被上限拒绝（accepted={accepted}）");
        assert!(accepted <= 64 * (WRITER_QUEUE_CAP / (64 * 1024)) + WRITER_QUEUE_CAP,
            "接受量不应远超上限（accepted={accepted}）");
        assert!(w.queued_bytes() <= WRITER_QUEUE_CAP, "排队字节不得超过上限");
    }

    #[test]
    fn order_is_preserved() {
        let sink = Sink::default();
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        for i in 0..50u8 {
            w.enqueue_typed(&[i]);
        }
        wait_bytes(&sink, 50);
        let got = sink.got.lock().unwrap().clone();
        assert_eq!(got, (0..50u8).collect::<Vec<_>>(), "写出顺序应与入队一致");
    }

    #[test]
    fn ctrl_c_drops_pending_paste_but_keeps_typed() {
        let sink = Sink::default();
        // 用不消费的写端先把队列堵住：注入 WouldBlock 之外的阻塞无法模拟，
        // 改为在 close 之前不入队任何可被快速消费的内容——直接检查队列状态。
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        // 填满队列使后续粘贴留在队列内
        let big = vec![b'p'; 64 * 1024];
        while w.try_enqueue_paste(&big) > 0 {}
        w.enqueue_typed(b"typed-before");
        w.enqueue_typed(&[0x03]);
        let st = w.shared.lock();
        let paste_left = st.items.iter().filter(|i| i.paste).count();
        let typed: Vec<u8> = st
            .items
            .iter()
            .filter(|i| !i.paste)
            .flat_map(|i| i.bytes.clone())
            .collect();
        assert_eq!(paste_left, 0, "Ctrl-C 后队列中不应再有粘贴数据");
        assert!(typed.ends_with(&[0x03]), "0x03 应已入队且位于末尾：{typed:?}");
        assert!(typed.starts_with(b"typed-before"), "键入数据应保留");
        drop(st);
        drop(w);
    }

    #[test]
    fn write_error_sets_failed_without_panic() {
        let sink = Sink { fail_kind: Some(ErrorKind::BrokenPipe), ..Default::default() };
        let w = PtyWriter::spawn(Box::new(sink));
        w.enqueue_typed(b"boom");
        for _ in 0..200 {
            if w.failed() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(w.failed(), "写端 EPIPE 应置 failed 状态（不 panic）");
        // 失败后不再接受入队
        assert_eq!(w.try_enqueue_paste(b"x"), 0);
        assert_eq!(w.queued_bytes(), 0, "失败应清空队列");
    }

    #[test]
    fn close_flushes_then_releases() {
        let sink = Sink::default();
        let mut w = PtyWriter::spawn(Box::new(sink.clone()));
        w.enqueue_typed(b"tail");
        w.close();
        let got = sink.got.lock().unwrap().clone();
        assert_eq!(got, b"tail".to_vec(), "close 应把队列写空后退出");
        assert!(!w.failed());
    }
}
