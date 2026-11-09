//! tty 终端入口：起 App 并运行事件循环。
//!
//! shell 取自环境变量 SHELL，未设置时回退 /bin/sh。

fn main() -> Result<(), app::Error> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| app::default_shell().to_string());
    let app = app::App::new(80, 24, shell);
    app.run()
}
