//! tty 终端入口：起 App 并运行事件循环。
//!
//! 配置加载（均为外部输入，失败回落默认，不 panic）：
//! - 路径优先级：--config <路径> > LEANTTY_CONFIG 环境变量 > 默认配置文件路径
//! - 字段优先级：命令行 --font/--font-size > APP_* 环境变量 > 配置文件 > 默认值
//! - 文件缺失 / 解析硬错误：stderr 报告后以默认配置启动
//! - 字段级问题（类型错误、颜色非法、数值越界、未知字段）：stderr 警告，
//!   该字段回落默认，配置整体仍可用

/// 与库内一致：`[perf]` 诊断行受 `LEANTTY_PERF_FRAMES` 门控（阶段 4O t3）。
fn perf_on() -> bool {
    std::env::var_os("LEANTTY_PERF_FRAMES").is_some_and(|v| !v.is_empty())
}

macro_rules! eperf {
    ($($arg:tt)*) => {
        if perf_on() {
            eprintln!($($arg)*);
        }
    };
}

use std::path::PathBuf;

fn main() -> Result<(), app::Error> {
    // 阶段 3C 任务 7：进程入口分段。先记录进程启动时刻，再设"锚点"，
    // 据此算出"进程启动 → main 首行"的偏移（覆盖 loader/动态链接耗时）。
    let proc_start = app::process_start_time();
    app::start_time_set();
    // 渲染侧启动锚点（阶段 3A 任务 2：字体加载分段耗时按同一锚点打印）。
    render::mark_start();
    // 阶段 5A t2：参数解析必须发生在创建事件循环与任何显示连接之前，
    // 使 --help / --version 在无 DISPLAY 的环境下也能用。
    let (cli_config, cli_font, cli_font_size, cli_command) =
        match app::cli::parse_args(std::env::args_os().skip(1)) {
            Ok(app::cli::Cli::Help) => {
                let default = config::default_config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "<无法确定：HOME/XDG_CONFIG_HOME 未设置>".to_string());
                print!("{}", app::cli::help_text(&default));
                return Ok(());
            }
            Ok(app::cli::Cli::Version) => {
                println!("leantty {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Ok(app::cli::Cli::Run {
                config_file,
                command,
                working_directory,
            }) => {
                // 阶段 7A a2：相对路径先绝对化（不用 canonicalize，避免解析符号链接）。
                let cfg = config_file.map(|p| {
                    let pb = PathBuf::from(p);
                    std::path::absolute(&pb).unwrap_or(pb)
                });
                if let Some(dir) = working_directory {
                    let orig = PathBuf::from(dir);
                    let pb = std::path::absolute(&orig).unwrap_or(orig);
                    match std::fs::metadata(&pb) {
                        Ok(m) if m.is_dir() => {}
                        Ok(_) => {
                            eprintln!("leantty: not a directory: {}", pb.display());
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("leantty: cannot use working directory '{}': {e}", pb.display());
                            std::process::exit(1);
                        }
                    }
                    if let Err(e) = std::env::set_current_dir(&pb) {
                        eprintln!("leantty: cannot enter working directory '{}': {e}", pb.display());
                        std::process::exit(1);
                    }
                }
                (cfg, None, None, command)
            }
            Err(e) => {
                // 用法错误：一行说明 + "Try --help"，退出码 2（不开窗口、不建事件循环）。
                eprintln!("leantty: {e}");
                eprintln!("Try --help");
                std::process::exit(2);
            }
        };
    // 阶段 5A t4（仅 Windows）：若当前进程是控制台的唯一使用者，释放控制台，
    // 避免双击启动时残留一个黑框。必须在参数解析成功、且不是 help/version 之后，
    // 创建事件循环之前执行（失败一律忽略）。
    #[cfg(windows)]
    app::free_if_sole_user();
    if let (Some(p), Some(t0)) = (proc_start, app::start_time()) {
        eperf!(
            "[perf] main_ms={:.1} since_start={:.1}",
            t0.duration_since(p).as_secs_f64() * 1000.0,
            t0.elapsed().as_secs_f64() * 1000.0
        );
    } else {
        eperf!("[perf] main_ms=unknown（无法读取进程启动时刻）");
    }
    let t_cfg = std::time::Instant::now();
    let config = load_config(cli_config, cli_font, cli_font_size);
    eperf!(
        "[perf] config_ms={:.1} since_start={:.1}",
        t_cfg.elapsed().as_secs_f64() * 1000.0,
        app::start_time()
            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    );
    let t_app = std::time::Instant::now();
    let app = app::App::new_with_command(config, cli_command);
    // 阶段 4H t2：App::new（配置归一化 + 字体设置解析 + 事件循环句柄）到 run 之间的分段点。
    eperf!(
        "[perf] app_new_ms={:.1} since_start={:.1}",
        t_app.elapsed().as_secs_f64() * 1000.0,
        app::start_time()
            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    );
    app.run()
}

/// 按优先级组装配置：命令行 > 环境变量 > 配置文件 > 默认值。
/// 任何一层失败均 stderr 报告并继续（不 panic）。
fn load_config(
    cli_config: Option<PathBuf>,
    cli_font: Option<String>,
    cli_font_size: Option<f32>,
) -> config::Config {
    // 阶段 5A t2：显式 `--config-file` 指向不可读文件 → 报错退出 1（不开窗口）；
    // 环境变量与默认路径沿用既有行为（stderr 警告后按默认配置启动）。
    let explicit = cli_config.clone();
    let path = match &explicit {
        Some(p) => Ok(p.clone()),
        None => config::resolve_config_path(None),
    };
    let mut config = match path {
        Ok(path) => match std::fs::read_to_string(&path) {
            Ok(text) => match config::parse(&text) {
                Ok((c, warnings)) => {
                    for w in warnings {
                        eprintln!("config: {w}");
                    }
                    c
                }
                Err(e) => {
                    if explicit.is_some() {
                        eprintln!("leantty: 无法解析配置文件 '{}': {e}", path.display());
                        std::process::exit(1);
                    }
                    eprintln!("config: {e}; using defaults");
                    config::Config::default()
                }
            },
            Err(e) => {
                if explicit.is_some() {
                    eprintln!("leantty: 无法读取配置文件 '{}': {e}", path.display());
                    std::process::exit(1);
                }
                eprintln!("config: {} 未读取 ({e}); using defaults", path.display());
                config::Config::default()
            }
        },
        Err(e) => {
            eprintln!("config: {e}; using defaults");
            config::Config::default()
        }
    };
    // 环境变量层（覆盖配置文件值）
    if let Some(cols) = env_u16("LEANTTY_COLS") {
        config.window.dimensions.columns = cols;
    }
    if let Some(rows) = env_u16("LEANTTY_ROWS") {
        config.window.dimensions.lines = rows;
    }
    if let Some(family) = env_string("LEANTTY_FONT_FAMILY") {
        config.font.normal.family = family;
    }
    if let Some(size) = env_f32("LEANTTY_FONT_SIZE") {
        config.font.size = size;
    }
    // 命令行层（最高优先级）
    if let Some(family) = cli_font {
        config.font.normal.family = family;
    }
    if let Some(size) = cli_font_size {
        config.font.size = size;
    }
    config
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
