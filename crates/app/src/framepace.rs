//! 自适应帧间隔（阶段 3C 任务 4）。
//!
//! 背景：固定 8ms 节流小于单帧渲染+present 耗时，帧仍背靠背执行（滚动输出时
//! 渲染占空比 74%–96%）。这里按"最近渲染耗时的 EMA × K"决定目标帧间隔，
//! 使渲染占空比稳定在约 1/K，并把间隔 clamp 到 [MIN, MAX]。
//!
//! 纯函数 + 状态机分离：`target_interval` 是纯函数（可单测），
//! `FramePacer` 只保存 EMA 与上次渲染时刻。

use std::time::Duration;

/// 渲染耗时 EMA 系数（新值权重）。
pub const PACE_EMA_ALPHA: f64 = 0.3;
/// 帧间隔相对渲染耗时的倍数（K=3 → 渲染占空比约 33%，含 present 时更低）。
pub const PACE_K: f64 = 3.0;
/// 帧间隔下限（用户输入路径同样受此下限约束，避免输入帧背靠背）。
pub const PACE_MIN_MS: f64 = 8.0;
/// 帧间隔上限（输出洪峰时最慢约 10fps，保证画面可感知推进）。
pub const PACE_MAX_MS: f64 = 100.0;

/// 计算目标帧间隔（纯函数）：`clamp(K × ema_ms, MIN, MAX)`。
///
/// ema_ms 为渲染耗时（毫秒）的指数移动平均；非有限值或非正数时回落到下限。
pub fn target_interval(ema_ms: f64) -> Duration {
    if !ema_ms.is_finite() || ema_ms <= 0.0 {
        return Duration::from_millis(PACE_MIN_MS as u64);
    }
    let ms = (PACE_K * ema_ms).clamp(PACE_MIN_MS, PACE_MAX_MS);
    Duration::from_secs_f64(ms / 1000.0)
}

/// 更新 EMA（纯函数）：alpha 固定 0.3；首次样本直接作为初值。
pub fn next_ema(prev: Option<f64>, sample_ms: f64) -> f64 {
    match prev {
        None => sample_ms,
        Some(p) => PACE_EMA_ALPHA * sample_ms + (1.0 - PACE_EMA_ALPHA) * p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_clamped_to_bounds() {
        // 极小渲染耗时 → 下限 8ms（不背靠背，也不无限加快）
        assert_eq!(target_interval(0.1).as_millis(), PACE_MIN_MS as u128);
        assert_eq!(target_interval(0.0).as_millis(), PACE_MIN_MS as u128);
        assert_eq!(target_interval(f64::NAN).as_millis(), PACE_MIN_MS as u128);
        // 极大渲染耗时 → 上限 100ms
        assert_eq!(target_interval(1000.0).as_millis(), PACE_MAX_MS as u128);
        // 中间值：K=3 → 3 × 20ms = 60ms
        assert_eq!(target_interval(20.0).as_millis(), 60);
    }

    #[test]
    fn ema_converges_and_reacts() {
        // 首次样本即初值
        assert_eq!(next_ema(None, 30.0), 30.0);
        // 第二个样本按 alpha=0.3 混合
        let e = next_ema(Some(30.0), 10.0);
        assert!((e - (0.3 * 10.0 + 0.7 * 30.0)).abs() < 1e-9, "ema={e}");
        // 收敛：同值样本反复输入后趋近该值
        let mut ema = Some(30.0);
        for _ in 0..50 {
            ema = Some(next_ema(ema, 10.0));
        }
        assert!(
            (ema.unwrap() - 10.0).abs() < 0.1,
            "应收敛到 10，实际 {ema:?}"
        );
    }

    #[test]
    fn interval_from_ema_sequence() {
        // 渲染耗时序列 5ms → 间隔 15ms；升到 40ms 后 EMA 未立刻跳满，间隔逐步增大
        let mut ema = next_ema(None, 5.0);
        assert_eq!(target_interval(ema).as_millis(), 15);
        ema = next_ema(Some(ema), 40.0);
        assert!(
            target_interval(ema).as_millis() > 15,
            "渲染变慢后间隔应增大"
        );
        assert!(target_interval(ema).as_millis() < 100, "且不超过上限");
        // 输出停止（渲染耗时回落）时间隔收敛回下限附近
        for _ in 0..60 {
            ema = next_ema(Some(ema), 0.5);
        }
        assert_eq!(target_interval(ema).as_millis(), 8);
    }
}
