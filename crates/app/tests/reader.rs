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
    let (tx, _rx) = mpsc::channel::<Vec<u8>>();
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
    let (tx, _rx) = mpsc::channel::<Vec<u8>>();
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
