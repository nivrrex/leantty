//! tty 终端入口：起 App 并运行事件循环。
//!
//! 配置来源（均为外部输入，解析失败回落默认，不 panic）：
//! - 环境变量 SHELL：shell 路径；未设置回落平台默认（app::default_shell）
//! - 环境变量 APP_COLS / APP_ROWS：网格行列，解析失败回落 80x24
//! - 环境变量 APP_FONT_FAMILY / APP_FONT_SIZE：字体覆盖
//! - 命令行 --font <family> / --font-size <pt>：覆盖环境变量（规格 2b 约定）

fn main() -> Result<(), app::Error> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| app::default_shell().to_string());
    let cols = env_u16("APP_COLS").unwrap_or(80);
    let rows = env_u16("APP_ROWS").unwrap_or(24);
    let mut app = app::App::new(cols, rows, shell);

    // 字体覆盖：环境变量为基础，命令行参数优先。
    let mut font_family = env_string("APP_FONT_FAMILY");
    let mut font_size = env_f32("APP_FONT_SIZE");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--font" => {
                if let Some(v) = args.next() {
                    font_family = Some(v);
                }
            }
            "--font-size" => {
                if let Some(v) = args.next() {
                    font_size = v.parse().ok();
                }
            }
            _ => {}
        }
    }
    app.set_font(font_family, font_size);
    app.run()
}

/// 读环境变量为 u16；未设置或非数值回落 None。
fn env_u16(name: &str) -> Option<u16> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// 读环境变量为 f32；未设置或非数值回落 None。
fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

/// 读环境变量为 String；未设置回落 None。
fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
