//! config crate 测试：examples 无警告加载、严格校验、绑定表、路径优先级。
use std::str::FromStr;

use config::{Action, Config, Mouse, Rgb};

const EXAMPLES: &[(&str, &str)] = &[
    ("linux", include_str!("../../../examples/linux.toml")),
    ("windows", include_str!("../../../examples/windows.toml")),
];

#[test]
fn examples_load_without_warnings() {
    for (name, text) in EXAMPLES {
        let (c, warnings) = config::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(warnings.is_empty(), "{name} warnings: {warnings:?}");
        assert_eq!(c.window.dimensions.columns, 120);
        assert_eq!(c.window.dimensions.lines, 30);
        assert_eq!(c.window.position, Some(config::Pos { x: 50, y: 40 }));
        assert_eq!(c.window.padding, config::Padding { x: 0, y: 0 });
        assert!(c.window.dynamic_title);
        assert_eq!(c.window.opacity, 1.0);
        assert_eq!(c.scrolling.history, 10000);
        assert_eq!(c.scrolling.multiplier, 10.0);
        assert!(c.scrolling.alt_screen_scroll, "默认 true（备用屏滚轮→方向键）");
        assert!(c.selection.save_to_clipboard);
        assert!(c.colors.draw_bold_text_with_bright_colors);
        assert_eq!(c.font.fallback.len(), 1);
        assert_eq!(c.mouse.bindings.len(), 2);
        assert_eq!(c.keyboard.bindings.len(), 2);
        if *name == "windows" {
            assert_eq!(
                c.terminal.shell.as_ref().unwrap().program,
                "pwsh.exe"
            );
            // 阶段 3A 任务 1：Windows 样例主字体改为小体积等宽西文 Consolas，
            // 大体积 CJK 字体（微软雅黑）放 fallback（主字体同步加载，大字体拖慢首帧）。
            assert_eq!(c.font.normal.family, "Consolas");
            assert_eq!(c.font.bold.family, "Consolas");
            assert_eq!(c.font.bold_italic.family, "Consolas");
            assert_eq!(c.font.fallback[0].family, "Microsoft YaHei");
            assert_eq!(c.colors.primary.background, Rgb { r: 0xfc, g: 0xfc, b: 0xfc });
        } else {
            assert_eq!(c.font.normal.family, "Consolas");
            assert_eq!(c.font.fallback[0].family, "黑体");
            assert_eq!(c.colors.primary.background, Rgb { r: 0xf3, g: 0xf1, b: 0xe9 });
        }
    }
}

#[test]
fn unknown_fields_warn_with_path() {
    let (_, w) = config::parse("[window]\nfoo = 1\n").unwrap();
    assert!(
        w.iter().any(|x| x.contains("window.foo")),
        "warnings: {w:?}"
    );
}

#[test]
fn bad_color_falls_back_with_warning() {
    let (c, w) = config::parse("[colors.primary]\nbackground = \"notacolor\"\n").unwrap();
    assert!(
        w.iter().any(|x| x.contains("colors.primary.background")),
        "warnings: {w:?}"
    );
    assert_eq!(c.colors.primary.background, Rgb { r: 0, g: 0, b: 0 });
}

#[test]
fn out_of_range_and_type_errors_fall_back() {
    let (c, w) = config::parse(
        "[window]\nopacity = 1.5\n[window.dimensions]\ncolumns = 0\n[font]\nsize = \"big\"\n",
    )
    .unwrap();
    assert_eq!(c.window.opacity, 1.0);
    assert_eq!(c.window.dimensions.columns, 80);
    assert_eq!(c.font.size, 14.0);
    assert!(w.iter().any(|x| x.contains("opacity")), "{w:?}");
    assert!(w.iter().any(|x| x.contains("columns")), "{w:?}");
    assert!(w.iter().any(|x| x.contains("size")), "{w:?}");
}

#[test]
fn bindings_parsed_with_mods() {
    let (c, w) = config::parse(
        "[[keyboard.bindings]]\nkey = \"Insert\"\nmods = \"Control|Shift\"\naction = \"Copy\"\n",
    )
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    let b = &c.keyboard.bindings[0];
    assert_eq!(b.key.as_deref(), Some("Insert"));
    assert_eq!(b.action, Action::Copy);
    assert!(b.mods.control && b.mods.shift && !b.mods.alt);
}

#[test]
fn invalid_binding_skipped_with_warning() {
    let (c, w) = config::parse("[[mouse.bindings]]\nmouse = \"Wheel\"\naction = \"Copy\"\n").unwrap();
    assert!(c.mouse.bindings.is_empty());
    assert!(w.iter().any(|x| x.contains("mouse")), "{w:?}");
}

#[test]
fn action_and_mouse_parsing() {
    assert_eq!(Action::from_str("PasteSelection"), Ok(Action::PasteSelection));
    assert_eq!(Action::from_str("None"), Ok(Action::None));
    assert_eq!(Action::from_str("Bogus"), Err(()));
    assert_eq!(Mouse::from_str("Middle"), Ok(Mouse::Middle));
    assert_eq!(Mouse::from_str("Wheel"), Err(()));
    let m: config::Mods = "Control|Shift".parse().unwrap();
    assert!(m.control && m.shift && !m.alt);
}

#[test]
fn rgb_rejects_bad_formats() {
    assert!("#F3F1E9".parse::<Rgb>().is_ok());
    assert!("F3F1E9".parse::<Rgb>().is_err());
    assert!("#F3F1E".parse::<Rgb>().is_err());
    assert!("#GGHHII".parse::<Rgb>().is_err());
}

#[test]
fn defaults_are_sane() {
    let c = Config::default();
    assert_eq!(c.window.dimensions.columns, 80);
    assert_eq!(c.window.dimensions.lines, 24);
    assert_eq!(c.font.size, 14.0);
    assert!(c.font.fallback.is_empty());
    assert!(c.mouse.bindings.is_empty());
    assert!(c.keyboard.bindings.is_empty());
}

/// 环境变量路径测试（合并为单测试，避免并行测试竞争进程级 env）。
#[test]
fn path_priority_cli_env_default() {
    // 平台无关路径构造（Unix 的 /tmp 前缀在 Windows 下不成立）。
    let tmp = std::env::temp_dir();
    let cli = tmp.join("x.toml");
    let p = config::resolve_config_path(Some(cli.clone())).unwrap();
    assert_eq!(p, cli);
    unsafe {
        let env_path = tmp.join("app-config-test.toml");
        std::env::set_var("APP_CONFIG", env_path.to_str().expect("temp 路径应为 UTF-8"));
        let p = config::resolve_config_path(None).unwrap();
        assert_eq!(p, env_path);
        std::env::remove_var("APP_CONFIG");

        // 默认路径基准目录按平台不同：Windows 读 APPDATA，其他平台读
        // XDG_CONFIG_HOME（未设置则 $HOME/.config）。只在此测试内分支，
        // 不改被测逻辑。
        let name = config::app_name();
        #[cfg(windows)]
        {
            let appdata = tmp.join("appdata-test");
            std::env::set_var("APPDATA", appdata.to_str().expect("temp 路径应为 UTF-8"));
            let p = config::default_config_path().unwrap();
            assert!(
                p.starts_with(&appdata),
                "path {p:?} should start with {appdata:?} (Windows APPDATA)"
            );
            // 用 Path 组件比较（避免手写分隔符）。
            assert!(
                p.ends_with(std::path::Path::new(&name).join(format!("{name}.toml"))),
                "path {p:?} should end with {name}/<bin>.toml"
            );
            std::env::remove_var("APPDATA");
        }
        #[cfg(not(windows))]
        {
            let xdg = tmp.join("xdg-test");
            std::env::set_var("XDG_CONFIG_HOME", xdg.to_str().expect("temp 路径应为 UTF-8"));
            let p = config::default_config_path().unwrap();
            assert!(
                p.starts_with(&xdg),
                "path {p:?} should start with {xdg:?}"
            );
            assert!(
                p.ends_with(std::path::Path::new(&name).join(format!("{name}.toml"))),
                "path {p:?} should end with {name}/<bin>.toml"
            );
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
    assert!(!config::app_name().is_empty());
}

#[test]
fn renderer_backend_defaults_to_auto() {
    let (c, w) = config::parse("").expect("parse");
    assert!(w.is_empty());
    assert_eq!(c.renderer.backend, config::Backend::Auto);
}

#[test]
fn renderer_backend_accepts_all_values() {
    for (text, expect) in [
        ("[renderer]\nbackend = \"auto\"", config::Backend::Auto),
        ("[renderer]\nbackend = \"cpu\"", config::Backend::Cpu),
        ("[renderer]\nbackend = \"gpu\"", config::Backend::Gpu),
    ] {
        let (c, w) = config::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert!(w.is_empty(), "{text}: {w:?}");
        assert_eq!(c.renderer.backend, expect, "{text}");
    }
}

#[test]
fn renderer_backend_invalid_value_warns_and_falls_back() {
    let (c, w) = config::parse("[renderer]\nbackend = \"vulkan\"").expect("parse");
    assert_eq!(c.renderer.backend, config::Backend::Auto);
    assert!(
        w.iter().any(|m| m.contains("renderer.backend")),
        "应带路径警告: {w:?}"
    );
}
