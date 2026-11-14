//! "输入发往 PTY 时是否把视口回到底部"的纯决策（阶段 4K t1b）。
//!
//! ## 规格（DECISIONS 同步登记）
//! 视口停在历史位置时，**向 PTY 发送内容**的操作应使视口回到底部；而"不向 PTY
//! 发送内容"的交互（仅修饰键、复制、滚轮/滚动条、选区、缩放）不应改变视口。
//!
//! ## 为什么单独抽这个函数
//! 修复前 app 里**没有任何**"输入→回底"的逻辑：回底只由"新输出到达且
//! `follow_bottom` 为真"触发（`about_to_wait` 的 auto-follow 分支），而用滚轮上滚
//! 会把 `follow_bottom` 置 false，于是键入只写 PTY、视口停在历史位置。
//! 抽出纯函数后，所有输入路径共用同一判据，并可单测覆盖边界（备用屏、鼠标上报）。

/// 触发点：调用方在**把字节写入 PTY 之前**报告它属于哪类交互。
///
/// 负向类别（仅修饰键/复制/滚轮/选区/缩放）不在生产路径上构造——它们的存在是为了
/// 让"决策边界"可被单测逐条固定，避免日后有人把这些交互也接进回底路径。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// 可打印字符键入。
    Printable,
    /// 编码为 PTY 输入的按键（Enter/Backspace/Tab/方向键等）。
    EncodedKey,
    /// 粘贴（右键、Ctrl+Shift+V、配置绑定动作；含括号粘贴包裹后的内容）。
    Paste,
    /// 输入法上屏提交。
    ImeCommit,
    /// 仅修饰键（不产生字节）。
    ModifierOnly,
    /// 复制动作（不产生字节）。
    Copy,
    /// 滚轮 / 滚动条拖动。
    Scroll,
    /// 选区操作。
    Selection,
    /// 窗口缩放。
    Resize,
}

impl InputEvent {
    /// 该类交互是否真的会向 PTY 写字节。
    pub fn sends_bytes(self) -> bool {
        matches!(
            self,
            InputEvent::Printable | InputEvent::EncodedKey | InputEvent::Paste | InputEvent::ImeCommit
        )
    }
}

/// 是否应在写入 PTY 之前把视口拉回底部。
///
/// - 只有"会向 PTY 写字节"的交互才考虑回底；
/// - 备用屏没有 scrollback，行为保持不变（不回底，也不产生副作用）；
/// - 鼠标上报模式下行为保持不变（滚轮由应用层消费为上报序列，视口由应用自己管）。
pub fn should_follow(ev: InputEvent, alt_screen: bool, mouse_reporting: bool) -> bool {
    ev.sends_bytes() && !alt_screen && !mouse_reporting
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALT: bool = true;
    const MAIN: bool = false;
    const MOUSE: bool = true;
    const NO_MOUSE: bool = false;

    #[test]
    fn pty_input_follows_on_primary_screen() {
        for ev in [
            InputEvent::Printable,
            InputEvent::EncodedKey,
            InputEvent::Paste,
            InputEvent::ImeCommit,
        ] {
            assert!(should_follow(ev, MAIN, NO_MOUSE), "{ev:?} 应回底");
        }
    }

    #[test]
    fn non_pty_interactions_never_follow() {
        for ev in [
            InputEvent::ModifierOnly,
            InputEvent::Copy,
            InputEvent::Scroll,
            InputEvent::Selection,
            InputEvent::Resize,
        ] {
            assert!(!should_follow(ev, MAIN, NO_MOUSE), "{ev:?} 不应回底");
        }
    }

    #[test]
    fn alternate_screen_keeps_behaviour() {
        for ev in [
            InputEvent::Printable,
            InputEvent::EncodedKey,
            InputEvent::Paste,
            InputEvent::ImeCommit,
        ] {
            assert!(!should_follow(ev, ALT, NO_MOUSE), "备用屏内 {ev:?} 不应触发回底");
        }
    }

    #[test]
    fn mouse_reporting_keeps_behaviour() {
        // 鼠标上报模式（TUI 应用自己管视口）：不因输入跳视口，既有行为不变。
        for ev in [
            InputEvent::Printable,
            InputEvent::EncodedKey,
            InputEvent::Paste,
            InputEvent::ImeCommit,
            InputEvent::Scroll,
        ] {
            assert!(!should_follow(ev, MAIN, MOUSE), "鼠标上报模式下 {ev:?} 不应回底");
        }
    }

    #[test]
    fn sends_bytes_only_for_input_class() {
        assert!(InputEvent::Printable.sends_bytes());
        assert!(!InputEvent::Copy.sends_bytes());
        assert!(!InputEvent::Scroll.sends_bytes());
    }
}
