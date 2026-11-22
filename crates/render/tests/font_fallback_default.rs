//! 默认回退链播种规则（0.2.10+）。
//!
//! 只断言"配置 → 生效链"的纯函数：`FontSet::load` 对缺失 face 的跳过属运行时行为，
//! 与机器装了什么字体有关，不在这里断言。

use render::FontSpec;
use render::fontset::effective_fallback;

fn spec(family: &str) -> FontSpec {
    FontSpec {
        family: family.to_string(),
        style: "Regular".to_string(),
    }
}

fn names(list: &[FontSpec]) -> Vec<&str> {
    list.iter().map(|s| s.family.as_str()).collect()
}

#[test]
fn empty_config_seeds_platform_default_chain_in_order() {
    let got = effective_fallback(&[]);
    let expected: &[&str] = if cfg!(windows) {
        &[
            "Segoe UI Symbol",
            "Segoe UI Emoji",
            "Microsoft YaHei",
            "MS Gothic",
        ]
    } else {
        &["Symbola", "DejaVu Sans", "Noto Sans Symbols2"]
    };
    assert_eq!(
        names(&got),
        expected,
        "空配置应播种当前平台的内置回退链，且顺序与实现约定一致"
    );
    assert!(
        got.iter().all(|s| s.style == "Regular"),
        "默认链每项都应是 Regular 样式"
    );
}

#[test]
fn platform_default_chain_is_exposed_for_callers() {
    let families = render::fontset::default_fallback_families();
    if cfg!(windows) {
        assert_eq!(families.len(), 4);
        assert_eq!(families[0], "Segoe UI Symbol", "符号字体必须在最前");
        assert!(
            families.contains(&"Microsoft YaHei"),
            "Windows 链必须含 CJK 字体"
        );
    } else {
        assert_eq!(families, ["Symbola", "DejaVu Sans", "Noto Sans Symbols2"]);
    }
}

#[test]
fn user_config_is_used_verbatim_without_appended_defaults() {
    let configured = [spec("DejaVu Sans"), spec("Noto Sans CJK SC")];
    let got = effective_fallback(&configured);
    assert_eq!(
        names(&got),
        ["DejaVu Sans", "Noto Sans CJK SC"],
        "用户显式配置时不得追加任何内置项"
    );
}

#[test]
fn user_config_containing_a_default_family_is_not_extended() {
    // 用户自己写了默认链里也有的家族：既不追加其余默认项，也不去重改写。
    let configured = [spec("Microsoft YaHei")];
    let got = effective_fallback(&configured);
    assert_eq!(names(&got), ["Microsoft YaHei"]);
}

#[test]
fn regular_style_loads_fonts_whose_face_is_named_book() {
    // DejaVu 系常规面的样式名是 "Book"：DirectWrite 按 "Regular" 精确匹配会 FontNotFound，
    // fontconfig 则会模糊匹配。加载层退化为描述式请求后，两种后端都应成功。
    let settings = render::FontSettings {
        size: 16.0,
        normal: spec("DejaVu Sans Mono"),
        bold: spec("DejaVu Sans Mono"),
        italic: spec("DejaVu Sans Mono"),
        bold_italic: spec("DejaVu Sans Mono"),
        fallback: Vec::new(),
    };
    assert!(
        render::FontSet::load(&settings).is_ok(),
        "样式名 Regular 应能落到家族默认面（DejaVu 的 Book）"
    );
}
