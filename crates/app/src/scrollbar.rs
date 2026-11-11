//! 滚动条几何（任务 4，纯函数可单测）。
//!
//! 视口占比模型：拇指高 = 轨道高 × viewport/(viewport+scrollback)，
//! 拇指顶 = 轨道高 × offset_from_top/scrollback（offset_from_top=0 在
//! scrollback 顶，=scrollback_rows 在底部/活动区）。

/// 滚动条拇指几何（轨道内坐标，px）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarGeom {
    /// 拇指顶（距轨道顶 px）。
    pub thumb_y: u32,
    /// 拇指高（px，min 1）。
    pub thumb_h: u32,
}

/// 滚动条几何计算；无 scrollback（视口即全部）返 None（不绘制滚动条）。
/// 外部输入校验：track_h=0 / viewport=0 / scrollback=0 均返 None。
pub fn scrollbar_geometry(
    scrollback_rows: u64,
    viewport_rows: u64,
    offset_from_top: usize,
    track_h: u32,
) -> Option<ScrollbarGeom> {
    if scrollback_rows == 0 || viewport_rows == 0 || track_h == 0 {
        return None;
    }
    // offset clamp 到 scrollback（外部输入：越界回落边界）。
    let offset = (offset_from_top as u64).min(scrollback_rows);
    let total = scrollback_rows + viewport_rows;
    let thumb_h = ((track_h as u64 * viewport_rows) / total).max(1) as u32;
    // 拇指顶 clamp 到 track_h-thumb_h（拇指不越轨道底；thumb_h≤track_h 恒成立）。
    let thumb_y = ((track_h as u64 * offset) / scrollback_rows)
        .min(track_h as u64 - thumb_h as u64) as u32;
    Some(ScrollbarGeom { thumb_y, thumb_h })
}

/// 像素 y（轨道内）→ scrollback 偏移行（拖拽定位）；逆映射 scrollbar_geometry。
pub fn track_y_to_offset(y: u32, track_h: u32, scrollback_rows: u64) -> usize {
    if track_h == 0 || scrollback_rows == 0 {
        return 0;
    }
    ((y as u64 * scrollback_rows) / track_h as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_none_when_no_scrollback() {
        assert_eq!(scrollbar_geometry(0, 24, 0, 300), None, "无 scrollback 不绘制");
        assert_eq!(scrollbar_geometry(100, 24, 0, 0), None, "track_h=0 防御");
        assert_eq!(scrollbar_geometry(100, 0, 0, 300), None, "viewport=0 防御");
    }

    #[test]
    fn geometry_thumb_proportional() {
        // scrollback=100 viewport=24 track=300：拇指高=300×24/124≈58（向下取整）
        let g = scrollbar_geometry(100, 24, 0, 300).expect("应绘制");
        assert_eq!(g.thumb_h, 58, "拇指高=viewport/(viewport+scrollback)×轨道");
        assert_eq!(g.thumb_y, 0, "offset=0 时拇指在顶");
        // offset=100（底部）：thumb_y=300×100/100=300→clamp 299（轨道内）
        let g = scrollbar_geometry(100, 24, 100, 300).expect("应绘制");
        assert!(g.thumb_y <= 300 - g.thumb_h, "底部时拇指不越轨道");
    }

    #[test]
    fn geometry_offset_clamped() {
        // offset 越界（>scrollback）回落边界，不 panic。
        let g = scrollbar_geometry(10, 24, 999, 300).expect("应绘制");
        let at_bottom = scrollbar_geometry(10, 24, 10, 300).expect("应绘制");
        assert_eq!(g.thumb_y, at_bottom.thumb_y, "越界 offset 回落底部");
    }

    #[test]
    fn track_y_to_offset_inverse() {
        assert_eq!(track_y_to_offset(0, 300, 100), 0, "轨道顶=scrollback 顶");
        assert_eq!(track_y_to_offset(300, 300, 100), 100, "轨道底=底部");
        assert_eq!(track_y_to_offset(150, 300, 100), 50, "中点=半偏移");
        assert_eq!(track_y_to_offset(10, 0, 100), 0, "track_h=0 防御");
    }
}
