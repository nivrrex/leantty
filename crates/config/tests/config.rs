//! config crate 测试：examples 无警告加载、严格校验、绑定表、路径优先级。
use std::path::PathBuf;
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
            assert_eq!(c.font.normal.family, "Microsoft YaHei Mono");
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
    let p = config::resolve_config_path(Some(PathBuf::from("/tmp/x.toml"))).unwrap();
    assert_eq!(p, PathBuf::from("/tmp/x.toml"));
    unsafe {
        std::env::set_var("APP_CONFIG", "/tmp/app-config-test.toml");
        let p = config::resolve_config_path(None).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/app-config-test.toml"));
        std::env::remove_var("APP_CONFIG");

        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let p = config::default_config_path().unwrap();
        let name = config::app_name();
        assert!(
            p.starts_with("/tmp/xdg-test"),
            "path {p:?} should start with /tmp/xdg-test"
        );
        assert!(
            p.ends_with(format!("{name}/{name}.toml").as_str()),
            "path {p:?} should end with {name}/{name}.toml"
        );
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    assert!(!config::app_name().is_empty());
}
