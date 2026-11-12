//! reader_body 的 wake 错误分类测试：EventLoopClosed 静默退出，其他错误上报。
use std::io::Read;
use std::sync::mpsc;

use app::{reader_body, Error, Wake};

/// 模拟事件循环已关闭的 waker。
struct LoopClosedWaker;

impl Wake for LoopClosedWaker {
    fn wake(&self) -> Result<(), Error> {
        Err(Error::EventLoopClosed)
    }
}

/// 模拟其他错误的 waker。
struct OtherErrWaker;

impl Wake for OtherErrWaker {
    fn wake(&self) -> Result<(), Error> {
        Err(Error::App("proxy down".into()))
    }
}

/// 无限数据源：若读线程不退出则测试挂起（函数返回即证明已退出）。
struct InfiniteReader;

impl Read for InfiniteReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        buf[0] = b'x';
        Ok(1)
    }
}

#[test]
fn loop_closed_no_report_and_reader_exits() {
    let (tx, _rx) = mpsc::sync_channel::<Vec<u8>>(16);
    let mut reported: Vec<String> = Vec::new();
    let mut reader = InfiniteReader;
    let waker = LoopClosedWaker;
    reader_body(&mut reader, &tx, &waker, &mut |m| reported.push(m.to_string()));
    assert!(
        reported.is_empty(),
        "EventLoopClosed 不应上报，实测: {:?}",
        reported
    );
}

#[test]
fn other_error_reported_and_reader_exits() {
    let (tx, _rx) = mpsc::sync_channel::<Vec<u8>>(16);
    let mut reported: Vec<String> = Vec::new();
    let mut reader = InfiniteReader;
    let waker = OtherErrWaker;
    reader_body(&mut reader, &tx, &waker, &mut |m| reported.push(m.to_string()));
    assert_eq!(reported.len(), 1, "其他错误应上报一次");
    assert!(
        reported[0].contains("proxy down"),
        "上报内容应含错误信息，实测: {:?}",
        reported[0]
    );
}

/// 立即 EOF 的数据源。
struct EofReader;

impl Read for EofReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

/// 计数 waker（记录 wake 调用次数；AtomicUsize 线程安全：reader_body 经 &self 调用）。
struct CountingWaker {
    count: std::sync::atomic::AtomicUsize,
}

impl Wake for CountingWaker {
    fn wake(&self) -> Result<(), Error> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn eof_wakes_event_loop_before_exit() {
    // 根因回归（case11 偶发 5s 未退出）：reader EOF 退出前必须 wake 主线程，
    // 否则事件循环无新事件可等→about_to_wait 不触发→try_wait 轮询丢失→挂起。
    let (tx, _rx) = mpsc::sync_channel::<Vec<u8>>(16);
    let waker = CountingWaker {
        count: std::sync::atomic::AtomicUsize::new(0),
    };
    let mut reader = EofReader;
    reader_body(&mut reader, &tx, &waker, &mut |_m| {});
    assert_eq!(
        waker.count.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "EOF 退出前应 wake 一次"
    );
}

/// 有限数据源：读 n 次，每次返回 1 字节（值递增），之后 EOF。
struct LimitedReader {
    left: usize,
    next: u8,
}

impl Read for LimitedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        self.left -= 1;
        buf[0] = self.next;
        self.next = self.next.wrapping_add(1);
        Ok(1)
    }
}

/// 有界通道背压回归（阶段 3B 任务 2）：通道容量小于数据量时，读线程
/// 必须在通道满时唤醒主线程并重试，最终按序送达全部字节且不死锁
/// （此前"先阻塞 send 再 wake"会在 200k 行输出时死锁）。
#[test]
fn full_channel_retries_and_delivers_all_without_deadlock() {
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1);
    let waker = CountingWaker {
        count: std::sync::atomic::AtomicUsize::new(0),
    };
    let mut reader = LimitedReader {
        left: 200,
        next: 0,
    };
    // 消费者线程：每次取一条，模拟主线程稍慢（触发通道满 → 读线程重试路径）。
    let consumer = std::thread::spawn(move || {
        let mut got: Vec<u8> = Vec::new();
        while got.len() < 200 {
            match rx.recv() {
                Ok(v) => {
                    got.extend_from_slice(&v);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
        got
    });
    let mut reported: Vec<String> = Vec::new();
    reader_body(&mut reader, &tx, &waker, &mut |m| reported.push(m.to_string()));
    drop(tx);
    let got = consumer.join().expect("consumer");
    assert!(reported.is_empty(), "不应上报错误：{reported:?}");
    assert_eq!(got.len(), 200, "全部字节应送达");
    // 按序（值递增 0..200）
    for (i, b) in got.iter().enumerate() {
        assert_eq!(*b, (i % 256) as u8, "第 {i} 字节应保序");
    }
    assert!(
        waker.count.load(std::sync::atomic::Ordering::Relaxed) >= 200,
        "每次入队至少一次 wake（含通道满时重试前的唤醒）"
    );
}
