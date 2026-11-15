// 探针：量 [perf] 一行 eprintln! 在 Windows 上到不同目标的单位成本。
// 用法：probe_logcost <lines> ；stderr 由调用方重定向（文件 / NUL / 管道）。
// 输出（stdout）：每行成本的中位/最小/最大，单位微秒。
use std::io::Write;
use std::time::Instant;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    // 与 font_steps_us 行同量级的长度（约 200 字节）
    let payload = "family='Consolas' style='Regular' kind=file bytes=459180 meta_len=459180 ttc=false query_us=28 face_us=0 meta_us=301 read_us=524 settings_us=0 parse_us=22761 clone_us=6519 insert_us=6 steps_us=30139 since_start=293.4";
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        eprintln!("[perf] font_steps_us {}", payload);
        samples.push(t.elapsed().as_micros() as u64);
    }
    // 也测一个无格式化的 WriteFile 直写，区分格式化成本与写成本
    let mut raw = Vec::with_capacity(n);
    let stderr = std::io::stderr();
    for _ in 0..n {
        let t = Instant::now();
        let mut h = stderr.lock();
        let _ = h.write_all(b"[perf] font_steps_us raw\n");
        let _ = h.flush();
        raw.push(t.elapsed().as_micros() as u64);
    }
    let med = |v: &Vec<u64>| {
        let mut s = v.clone();
        s.sort_unstable();
        s[s.len() / 2]
    };
    println!(
        "eprintln n={} med_us={} min_us={} max_us={} | rawwrite med_us={} min_us={} max_us={}",
        n,
        med(&samples),
        samples.iter().min().unwrap(),
        samples.iter().max().unwrap(),
        med(&raw),
        raw.iter().min().unwrap(),
        raw.iter().max().unwrap()
    );
}
