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
//! - **Ctrl-C（0x03）**：入队 0x03 前先丢弃队列中**尚未写出**的粘贴数据；
//!   Unix 写端非阻塞，若当前块已阻塞则丢弃在途粘贴、清空 PTY 输入队列，
//!   并由 app 在必要时中断前台进程组，使用户中断不被大粘贴卡住。
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
use std::time::Duration;

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
    /// 该块包含括号粘贴**起始**标记 `ESC[200~`（阶段 4I t2）。
    starts_bracketed: bool,
    /// 该块包含括号粘贴**结束**标记 `ESC[201~`（阶段 4I t2）。
    ends_bracketed: bool,
}

/// 括号粘贴标记（与 `paste::normalize_paste` 生成的序列一致）。
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// 共享状态（唯一可变状态，受互斥量保护）。
struct State {
    items: VecDeque<Item>,
    /// 队列中待写字节数（与 `items` 同步维护）。
    queued: usize,
    /// 写线程当前是否因 WouldBlock 等待对端读取。
    blocked: bool,
    /// 关闭中：写线程在**写空队列后**退出。
    closing: bool,
    /// 写端错误（EIO/EPIPE 等），非 None 后不再接受入队。
    failed: Option<std::io::ErrorKind>,
    /// 已开始写出括号粘贴起始标记、且结束标记尚未写出（阶段 4I t2）。
    open_paste: bool,
    /// 写线程当前在写的那一块本身带结束标记（此时不必补发）。
    in_flight_ends: bool,
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
        Self::spawn_with_fd(writer, None)
    }

    /// Unix 上传入主端 fd，写阻塞时可丢弃已排队输入，让 Ctrl-C 及时送达。
    pub fn spawn_with_fd(
        writer: Box<dyn Write + Send>,
        control_fd: Option<std::os::raw::c_int>,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                items: VecDeque::new(),
                queued: 0,
                closing: false,
                failed: None,
                open_paste: false,
                in_flight_ends: false,
                blocked: false,
            }),
            cv: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("pty-writer".to_string())
            .spawn(move || worker(writer, worker_shared, control_fd))
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
            let dropped: usize = st
                .items
                .iter()
                .filter(|it| it.paste)
                .map(|it| it.bytes.len())
                .sum();
            st.items.retain(|it| !it.paste);
            st.queued = st.items.iter().map(|it| it.bytes.len()).sum();
            // 阶段 4I t2 取证：Ctrl-C 时确实丢弃了"尚未写出"的粘贴数据。
            // 仅在该罕见路径打印（无热路径开销），release 同样可用。
            if dropped > 0 {
                eprintln!("[perf] ctrl_c_drop_paste bytes={dropped}");
            }
            // 阶段 4I t2 修复：bash 等 readline 前端收到 `ESC[200~` 后进入"粘贴内容"
            // 状态，只有 `ESC[201~` 才退出。若起始标记已写出、结束标记被上面的丢弃
            // 语句一并丢掉，shell 会永久卡在该状态（实测 5/5 复现：之后键入不被执行、
            // Ctrl-C 变成字面字符）。因此在 0x03 之前补发一个结束标记。
            //
            // 补发条件严格限定：起始已开始写、且正在写的那一块本身不带结束标记
            // （后者说明结束标记已在途，再补会造成多余的结束标记）。
            // 0x03 与结束标记同一临界区入队、紧随其后，不等待写出。
            if st.open_paste && !st.in_flight_ends {
                st.items.push_back(Item {
                    bytes: PASTE_END.to_vec(),
                    paste: false,
                    starts_bracketed: false,
                    ends_bracketed: true,
                });
                st.queued += PASTE_END.len();
                st.open_paste = false;
                eprintln!("[perf] ctrl_c_restore_paste_end");
            }
        }
        st.queued += bytes.len();
        st.items.push_back(Item {
            bytes: bytes.to_vec(),
            paste: false,
            starts_bracketed: false,
            ends_bracketed: false,
        });
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
        let chunk = &bytes[..n];
        st.items.push_back(Item {
            bytes: chunk.to_vec(),
            paste: true,
            starts_bracketed: chunk.starts_with(PASTE_START),
            ends_bracketed: chunk.ends_with(PASTE_END),
        });
        self.shared.cv.notify_one();
        n
    }

    /// 写线程是否正等待 PTY 可写。
    pub fn is_blocked(&self) -> bool {
        self.shared.lock().blocked
    }

    /// 写端是否已失败（EIO/EPIPE 等）。调用方可据此走既有退出路径。
    pub fn failed(&self) -> bool {
        self.shared.lock().failed.is_some()
    }

    /// 当前排队字节数（观测用）。
    pub fn queued_bytes(&self) -> usize {
        self.shared.lock().queued
    }

    /// 队列当前的元素个数（观测用：阶段 4K t2 判断容量保留）。
    pub fn queued_len(&self) -> usize {
        self.shared.lock().items.len()
    }

    /// 队列容器的容量（观测用：VecDeque 的 capacity 是否随时间增长）。
    pub fn capacity(&self) -> usize {
        self.shared.lock().items.capacity()
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
fn worker(
    mut writer: Box<dyn Write + Send>,
    shared: Arc<Shared>,
    _control_fd: Option<std::os::raw::c_int>,
) {
    loop {
        let item = {
            let mut st = shared.lock();
            loop {
                if let Some(item) = st.items.pop_front() {
                    st.queued -= item.bytes.len();
                    st.in_flight_ends = item.ends_bracketed;
                    if item.starts_bracketed {
                        st.open_paste = true;
                    }
                    break Some(item);
                }
                if st.closing || st.failed.is_some() {
                    break None;
                }
                st = shared.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        let Some(item) = item else { return };
        let ends = item.ends_bracketed;
        let mut off = 0usize;
        while off < item.bytes.len() {
            let cancel = if item.paste {
                let mut st = shared.lock();
                let has_ctrl_c = st
                    .items
                    .iter()
                    .any(|it| !it.paste && it.bytes.len() == 1 && it.bytes[0] == 0x03);
                if has_ctrl_c {
                    st.items.retain(|it| !it.paste);
                    st.queued = st.items.iter().map(|it| it.bytes.len()).sum();
                }
                has_ctrl_c
            } else {
                false
            };
            if cancel {
                #[cfg(unix)]
                if let Some(fd) = _control_fd {
                    let _ = pty::flush_input(fd);
                }
                break;
            }
            match writer.write(&item.bytes[off..]) {
                Ok(0) => {
                    let mut st = shared.lock();
                    st.failed = Some(std::io::ErrorKind::WriteZero);
                    st.items.clear();
                    st.queued = 0;
                    shared.cv.notify_all();
                    return;
                }
                Ok(n) => {
                    {
                        let mut st = shared.lock();
                        st.blocked = false;
                    }
                    off += n;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    {
                        let mut st = shared.lock();
                        st.blocked = true;
                    }
                    #[cfg(unix)]
                    if let Some(fd) = _control_fd {
                        let _ = pty::wait_writable(fd, 10);
                    } else {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    #[cfg(not(unix))]
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Err(e) => {
                    let mut st = shared.lock();
                    st.failed = Some(e.kind());
                    st.items.clear();
                    st.queued = 0;
                    shared.cv.notify_all();
                    return;
                }
            }
        }
        if ends {
            let mut st = shared.lock();
            st.open_paste = false;
        }
    }
}

/// 测试支持：受控写端与放行闸门。
///
/// 只给测试用（`cfg(test)`），不进入任何构建产物。设计目的：让"写线程是否
/// 已经消费"完全由测试线程决定，断言只依赖队列自身状态，不再与消费线程的
/// 调度赛跑（阶段 4J t2）。
#[cfg(test)]
pub(crate) mod test_support {
    use std::io::Write;
    use std::sync::{Arc, Condvar, Mutex};

    /// 闸门：`false` 时 write 阻塞，`release()` 后放行（单向）。
    ///
    /// `Clone` 共享同一份状态：测试保留一份用于放行与读回，写端另一份交给线程。
    #[derive(Clone)]
    pub(crate) struct GatedSink {
        got: Arc<Mutex<Vec<u8>>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        fail_kind: Option<std::io::ErrorKind>,
    }

    impl GatedSink {
        /// 关闭状态的闸门默认不放行。
        pub(crate) fn new() -> Self {
            Self {
                got: Arc::new(Mutex::new(Vec::new())),
                gate: Arc::new((Mutex::new(false), Condvar::new())),
                fail_kind: None,
            }
        }

        /// 立即失败的闸门（用于写端错误路径）。
        pub(crate) fn failing(kind: std::io::ErrorKind) -> Self {
            Self {
                fail_kind: Some(kind),
                ..Self::new()
            }
        }

        /// 放行所有当前与后续 write。
        pub(crate) fn release(&self) {
            let (m, cv) = &*self.gate;
            *m.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cv.notify_all();
        }
    }

    impl Write for GatedSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(k) = self.fail_kind {
                return Err(std::io::Error::new(k, "注入错误"));
            }
            let (m, cv) = &*self.gate;
            let mut open = m.lock().unwrap_or_else(|e| e.into_inner());
            while !*open {
                open = cv.wait(open).unwrap_or_else(|e| e.into_inner());
            }
            drop(open);
            self.got
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::GatedSink;
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

    /// 等写线程取走首块并进入 `write`（受控 sink 关闭时它会阻塞在此）。
    /// 返回 true 表示队列已空（首块在途）——此后队列状态不再被消费改变。
    fn wait_consumed_first(w: &PtyWriter) -> bool {
        for _ in 0..400 {
            if w.queued_bytes() == 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        w.queued_bytes() == 0
    }

    fn wait_bytes(sink: &Sink, want: usize) {
        for _ in 0..200 {
            if sink.got.lock().unwrap().len() >= want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[derive(Clone)]
    struct WouldBlockSink {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        allow: Arc<std::sync::atomic::AtomicBool>,
        got: Arc<StdMutex<Vec<u8>>>,
    }

    impl Write for WouldBlockSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if !self.allow.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(std::io::Error::new(ErrorKind::WouldBlock, "pty full"));
            }
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn ctrl_c_cancels_a_blocked_in_flight_paste() {
        let sink = WouldBlockSink {
            calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            allow: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            got: Arc::new(StdMutex::new(Vec::new())),
        };
        let mut w = PtyWriter::spawn(Box::new(sink.clone()));
        assert!(w.try_enqueue_paste(b"PASTE") > 0);
        for _ in 0..200 {
            if sink.calls.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(sink.calls.load(std::sync::atomic::Ordering::SeqCst) > 0);
        w.enqueue_typed(&[0x03]);
        sink.allow.store(true, std::sync::atomic::Ordering::SeqCst);
        w.close();
        assert_eq!(
            &*sink.got.lock().unwrap(),
            &[0x03],
            "阻塞粘贴应被丢弃，Ctrl-C 必须写出"
        );
    }

    #[test]
    fn paste_queue_respects_cap() {
        // 受控 sink：写线程最多取走一块后阻塞在写入，队列状态对断言完全确定。
        // （旧版用即时消费的 Sink，断言"持续入队必被拒绝"依赖生产者比消费者快，
        //  在调度延迟更大时（Windows 实测 10 次失败 3 次）会失效。）
        let sink = GatedSink::new();
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        let chunk = vec![b'x'; 64 * 1024];
        // 先让写线程取走一块并阻塞，之后队列状态完全由本线程决定。
        assert!(w.try_enqueue_paste(&chunk) > 0);
        assert!(wait_consumed_first(&w), "写线程应已取走首块并阻塞");
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
        assert!(rejected, "队列满时应拒绝（accepted={accepted}）");
        // 确定性上下界：写线程至多已取走一块（在写入中），故接受量落在
        // [上限, 上限 + 一块] 内，与调度无关。
        assert!(
            accepted >= WRITER_QUEUE_CAP,
            "接受量应至少达到上限（accepted={accepted}）"
        );
        assert!(
            accepted <= WRITER_QUEUE_CAP + chunk.len(),
            "接受量不应超过上限 + 一块在途（accepted={accepted}）"
        );
        assert!(w.queued_bytes() <= WRITER_QUEUE_CAP, "排队字节不得超过上限");
        // 写线程阻塞期间队列保持满：再入队仍被拒绝（不依赖消费速度）。
        assert_eq!(w.try_enqueue_paste(&chunk), 0, "阻塞期间队列应保持满");
        sink.release();
        drop(w);
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
        // 受控 sink：写线程阻塞在首块写入，后续入队全部留在队列里，
        // "Ctrl-C 丢弃粘贴、保留键入"可被确定性观测（旧版依赖消费速度，可能
        // 在断言前就被消费掉）。
        let sink = GatedSink::new();
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        let big = vec![b'p'; 64 * 1024];
        assert!(w.try_enqueue_paste(&big) > 0, "首块应被接受");
        assert!(wait_consumed_first(&w), "写线程应已取走首块并阻塞");
        // 再灌一批粘贴数据，让 Ctrl-C 有东西可丢
        assert!(w.try_enqueue_paste(&big) > 0, "第二块应被接受");
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
        assert!(
            typed.ends_with(&[0x03]),
            "0x03 应已入队且位于末尾：{typed:?}"
        );
        assert!(
            typed.starts_with(b"typed-before"),
            "键入数据应保留：queue={:?}",
            st.items
                .iter()
                .map(|i| (i.paste, String::from_utf8_lossy(&i.bytes).to_string()))
                .collect::<Vec<_>>()
        );
        drop(st);
        sink.release();
        drop(w);
    }

    #[test]
    fn write_error_sets_failed_without_panic() {
        let w = PtyWriter::spawn(Box::new(GatedSink::failing(ErrorKind::BrokenPipe)));
        w.enqueue_typed(b"boom");
        // 失败路径不依赖闸门：写端立即返回 EPIPE。轮询仅为等待写线程收敛。
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

    /// 闸门写端：第一次 write 阻塞，直到测试放行；用于让"正在写的那一块"停在飞。
    #[derive(Clone)]
    struct GateSink {
        gate: Arc<(StdMutex<bool>, Condvar)>,
        got: Arc<StdMutex<Vec<u8>>>,
    }

    impl GateSink {
        fn new(open: bool) -> Self {
            Self {
                gate: Arc::new((StdMutex::new(open), Condvar::new())),
                got: Arc::new(StdMutex::new(Vec::new())),
            }
        }
        fn release(&self) {
            let (m, cv) = &*self.gate;
            *m.lock().unwrap() = true;
            cv.notify_all();
        }
    }

    impl Write for GateSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let (m, cv) = &*self.gate;
            let mut open = m.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            drop(open);
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 等写线程真正进入"写第一块"的状态（in_flight 已置）。
    fn wait_in_flight(w: &PtyWriter) {
        for _ in 0..200 {
            {
                let st = w.shared.lock();
                if st.open_paste || st.in_flight_ends {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("写线程未进入在写状态");
    }

    #[test]
    fn ctrl_c_restores_paste_end_when_start_already_writing() {
        let sink = GateSink::new(false);
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        // 起始块：worker 取走后阻塞在 write 里（结束标记尚未写出）
        w.try_enqueue_paste(b"\x1b[200~start-of-paste");
        wait_in_flight(&w);
        // 后续块仍在队列里，会被 Ctrl-C 丢弃
        assert!(w.try_enqueue_paste(b"middle-chunk-1") > 0);
        w.enqueue_typed(&[0x03]);
        {
            let st = w.shared.lock();
            assert!(
                st.items.iter().all(|it| !it.paste),
                "Ctrl-C 后队列中不应再有粘贴块"
            );
            let bytes: Vec<u8> = st.items.iter().flat_map(|it| it.bytes.clone()).collect();
            assert!(
                bytes.starts_with(b"\x1b[201~"),
                "应先补发括号粘贴结束标记，实测队列={bytes:?}"
            );
            assert_eq!(bytes[PASTE_END.len()..], [0x03], "结束标记后应紧跟 0x03");
        }
        sink.release();
        drop(w);
    }

    #[test]
    fn no_restore_when_no_paste_started() {
        let sink = GateSink::new(true);
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        w.enqueue_typed(b"plain");
        w.enqueue_typed(&[0x03]);
        {
            let st = w.shared.lock();
            let bytes: Vec<u8> = st.items.iter().flat_map(|it| it.bytes.clone()).collect();
            assert!(
                !bytes.windows(PASTE_END.len()).any(|w| w == PASTE_END),
                "从未开始括号粘贴时不应补发结束标记：{bytes:?}"
            );
        }
        drop(w);
    }

    #[test]
    fn no_restore_when_in_flight_chunk_ends_with_marker() {
        let sink = GateSink::new(false);
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        // 单块粘贴（起始 + 结束在同一块）：结束标记已在途，不应再补
        w.try_enqueue_paste(b"\x1b[200~all\x1b[201~");
        wait_in_flight(&w);
        w.enqueue_typed(&[0x03]);
        {
            let st = w.shared.lock();
            let bytes: Vec<u8> = st.items.iter().flat_map(|it| it.bytes.clone()).collect();
            assert_eq!(bytes, vec![0x03], "结束标记已在途时不应重复补发：{bytes:?}");
        }
        sink.release();
        drop(w);
    }

    #[test]
    fn restore_once_for_multiple_dropped_pastes() {
        let sink = GateSink::new(false);
        let w = PtyWriter::spawn(Box::new(sink.clone()));
        w.try_enqueue_paste(b"\x1b[200~p1");
        wait_in_flight(&w);
        // 两个粘贴的余下块堆在队列里，全部会被丢弃
        w.try_enqueue_paste(b"p1-more");
        w.try_enqueue_paste(b"\x1b[200~p2");
        w.try_enqueue_paste(b"p2-more\x1b[201~");
        w.enqueue_typed(&[0x03]);
        {
            let st = w.shared.lock();
            let ends: usize = st
                .items
                .iter()
                .filter(|it| it.bytes == PASTE_END.to_vec())
                .count();
            assert_eq!(ends, 1, "多次丢弃只补一个结束标记");
        }
        sink.release();
        drop(w);
    }
}
