//! 配置系统（阶段 2A 任务 1）。
//!
//! - 格式 TOML，字段名与 alacritty 对齐（含本仓库新增 font.fallback 有序列表）。
//! - 解析与应用解耦：本 crate 只产出强类型 Config，不依赖 winit/render/vt；
//!   加载函数接收字符串，文件 I/O 在外层（app），便于单测。
//! - 严格校验：未知字段给带路径警告（不静默忽略）；类型错误、颜色格式非法、
//!   数值越界给清晰警告并使该字段回落默认值，全程不 panic。
//! - 选型（偏差登记于 DECISIONS.md）：未用 serde derive 镜像结构——derive 的
//!   类型错误是整表失败，不满足"字段级回落默认"；改用手写 toml::Value 逐字段
//!   解析层（toml crate 本身基于 serde）。
//! - 不做热重载、import、per-shell 配置。

use std::path::PathBuf;
use std::str::FromStr;

use thiserror::Error;

/// 配置加载硬错误（TOML 语法等）：外层回落默认配置并在 stderr 报告。
#[derive(Debug, Error)]
pub enum Error {
    #[error("config parse: {0}")]
    Parse(String),
    #[error("config path: {0}")]
    Path(String),
}

/// 字段级问题（警告）：记录后该字段回落默认，配置整体仍可用。
pub type Warnings = Vec<String>;

/// 绑定动作（阶段 2A 只解析与校验，实际执行在 2B）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Copy,
    Paste,
    PasteSelection,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    None,
}

impl FromStr for Action {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Copy" => Ok(Action::Copy),
            "Paste" => Ok(Action::Paste),
            "PasteSelection" => Ok(Action::PasteSelection),
            "ScrollPageUp" => Ok(Action::ScrollPageUp),
            "ScrollPageDown" => Ok(Action::ScrollPageDown),
            "ScrollToTop" => Ok(Action::ScrollToTop),
            "ScrollToBottom" => Ok(Action::ScrollToBottom),
            "None" => Ok(Action::None),
            _ => Err(()),
        }
    }
}

/// 鼠标键（mouse.bindings 的 mouse 字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    Left,
    Middle,
    Right,
}

impl FromStr for Mouse {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Left" => Ok(Mouse::Left),
            "Middle" => Ok(Mouse::Middle),
            "Right" => Ok(Mouse::Right),
            _ => Err(()),
        }
    }
}

/// 修饰键位集（keyboard.bindings 的 mods 字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_: bool,
    pub hyper: bool,
    pub meta: bool,
}

impl FromStr for Mods {
    type Err = ();
    /// 接受单个修饰键或 "|" 组合（如 "Control|Shift"）；空串为空集。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut mods = Mods::default();
        if s.is_empty() {
            return Ok(mods);
        }
        for part in s.split('|') {
            match part {
                "Control" => mods.control = true,
                "Shift" => mods.shift = true,
                "Alt" => mods.alt = true,
                "Super" => mods.super_ = true,
                "Hyper" => mods.hyper = true,
                "Meta" => mods.meta = true,
                _ => return Err(()),
            }
        }
        Ok(mods)
    }
}

/// 颜色（#rrggbb）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl FromStr for Rgb {
    type Err = ();
    /// 仅接受 #rrggbb（7 字符、小写或大写 hex 均可）。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let b = s.as_bytes();
        if b.len() != 7 || b[0] != b'#' {
            return Err(());
        }
        let hex = |i: usize| {
            u8::from_str_radix(std::str::from_utf8(&b[i..i + 2]).map_err(|_| ())?, 16)
                .map_err(|_| ())
        };
        Ok(Rgb {
            r: hex(1)?,
            g: hex(3)?,
            b: hex(5)?,
        })
    }
}

/// 字体描述（family + style）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSpec {
    pub family: String,
    pub style: String,
}

/// 窗口位置（逻辑像素，可负）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub x: i64,
    pub y: i64,
}

/// 初始网格行列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub columns: u16,
    pub lines: u16,
}

/// 内边距（逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Padding {
    pub x: u16,
    pub y: u16,
}

/// 窗口配置。
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub position: Option<Pos>,
    pub dimensions: Dimensions,
    pub padding: Padding,
    pub dynamic_title: bool,
    /// 透明度（0..=1）；渲染层未实现 opacity 时保留字段（DECISIONS 登记）。
    pub opacity: f32,
}

/// 滚动配置。
#[derive(Debug, Clone, Copy)]
pub struct ScrollingConfig {
    pub history: u64,
    pub multiplier: f32,
    /// 备用屏滚轮→方向键（true：备用屏下滚轮发 Up/Down 键而非 scroll_viewport）。
    pub alt_screen_scroll: bool,
}

/// shell 配置。
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub program: String,
    /// 参数已解析并校验；PTY spawn 接口当前不含 args（DECISIONS 登记），接线待后续阶段。
    pub args: Vec<String>,
}

/// terminal 段。
#[derive(Debug, Clone)]
pub struct TerminalConfig {
    pub shell: Option<ShellConfig>,
}

/// 抗锯齿模式（font.antialiasing）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Antialiasing {
    Grayscale,
    None,
}

/// 亚像素模式（font.subpixel）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subpixel {
    None,
    Rgb,
    Bgr,
}

/// 字体配置。
#[derive(Debug, Clone)]
pub struct FontSettings {
    pub size: f32,
    pub normal: FontSpec,
    pub bold: FontSpec,
    pub italic: FontSpec,
    pub bold_italic: FontSpec,
    /// 有序回退列表（新增字段，alacritty 无此字段）。
    pub fallback: Vec<FontSpec>,
    pub antialiasing: Antialiasing,
    pub subpixel: Subpixel,
    pub hinting: bool,
    /// 文字对比度（抗锯齿覆盖率→alpha 映射）。
    pub text_gamma: f32,
}

/// 选择配置。
#[derive(Debug, Clone)]
pub struct SelectionConfig {
    pub save_to_clipboard: bool,
    pub semantic_escape_chars: String,
}

/// ANSI 8 色组。
#[derive(Debug, Clone, Copy)]
pub struct Ansi8 {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
}

/// 光标颜色。
#[derive(Debug, Clone, Copy)]
pub struct CursorColors {
    pub text: Rgb,
    pub cursor: Rgb,
}

/// 主色。
#[derive(Debug, Clone, Copy)]
pub struct PrimaryColors {
    pub background: Rgb,
    pub foreground: Rgb,
}

/// 颜色配置。
#[derive(Debug, Clone)]
pub struct Colors {
    pub draw_bold_text_with_bright_colors: bool,
    pub primary: PrimaryColors,
    pub cursor: CursorColors,
    pub normal: Ansi8,
    pub bright: Ansi8,
}

/// 单条绑定（mouse.bindings 与 keyboard.bindings 共用结构）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub mouse: Option<Mouse>,
    pub key: Option<String>,
    pub mods: Mods,
    pub action: Action,
}

/// 鼠标配置。
#[derive(Debug, Clone)]
pub struct MouseConfig {
    pub bindings: Vec<Binding>,
}

/// 键盘配置。
#[derive(Debug, Clone)]
pub struct KeyboardConfig {
    pub bindings: Vec<Binding>,
}

/// 顶层配置（强类型输出）。
#[derive(Debug, Clone)]
pub struct Config {
    pub window: WindowConfig,
    pub scrolling: ScrollingConfig,
    pub terminal: TerminalConfig,
    pub font: FontSettings,
    pub selection: SelectionConfig,
    pub colors: Colors,
    pub mouse: MouseConfig,
    pub keyboard: KeyboardConfig,
}

impl Default for Config {
    fn default() -> Self {
        let platform_family = if cfg!(target_os = "windows") {
            "Consolas"
        } else {
            "monospace"
        };
        Self {
            window: WindowConfig {
                position: None,
                dimensions: Dimensions {
                    columns: 80,
                    lines: 24,
                },
                padding: Padding { x: 0, y: 0 },
                dynamic_title: true,
                opacity: 1.0,
            },
            scrolling: ScrollingConfig {
                history: 10000,
                multiplier: 3.0,
                alt_screen_scroll: true,
            },
            terminal: TerminalConfig { shell: None },
            font: FontSettings {
                size: 14.0,
                normal: FontSpec {
                    family: platform_family.to_string(),
                    style: "Regular".to_string(),
                },
                bold: FontSpec {
                    family: platform_family.to_string(),
                    style: "Bold".to_string(),
                },
                italic: FontSpec {
                    family: platform_family.to_string(),
                    style: "Italic".to_string(),
                },
                bold_italic: FontSpec {
                    family: platform_family.to_string(),
                    style: "Bold Italic".to_string(),
                },
                fallback: Vec::new(),
                antialiasing: Antialiasing::Grayscale,
                subpixel: Subpixel::None,
                hinting: true,
                text_gamma: 1.0,
            },
            selection: SelectionConfig {
                save_to_clipboard: true,
                semantic_escape_chars: ",│`|:\"' ()[]{}<>".to_string(),
            },
            colors: Colors {
                draw_bold_text_with_bright_colors: false,
                primary: PrimaryColors {
                    background: Rgb {
                        r: 0,
                        g: 0,
                        b: 0,
                    },
                    foreground: Rgb {
                        r: 255,
                        g: 255,
                        b: 255,
                    },
                },
                cursor: CursorColors {
                    text: Rgb {
                        r: 255,
                        g: 255,
                        b: 255,
                    },
                    cursor: Rgb {
                        r: 0,
                        g: 0,
                        b: 0,
                    },
                },
                normal: Ansi8 {
                    black: Rgb { r: 0, g: 0, b: 0 },
                    red: Rgb { r: 205, g: 0, b: 0 },
                    green: Rgb { r: 0, g: 205, b: 0 },
                    yellow: Rgb { r: 205, g: 205, b: 0 },
                    blue: Rgb { r: 0, g: 0, b: 238 },
                    magenta: Rgb { r: 215, g: 0, b: 159 },
                    cyan: Rgb { r: 0, g: 205, b: 205 },
                    white: Rgb {
                        r: 229,
                        g: 229,
                        b: 229,
                    },
                },
                bright: Ansi8 {
                    black: Rgb {
                        r: 127,
                        g: 127,
                        b: 127,
                    },
                    red: Rgb { r: 255, g: 0, b: 0 },
                    green: Rgb { r: 0, g: 255, b: 0 },
                    yellow: Rgb { r: 255, g: 255, b: 0 },
                    blue: Rgb { r: 92, g: 92, b: 255 },
                    magenta: Rgb { r: 255, g: 0, b: 255 },
                    cyan: Rgb { r: 0, g: 255, b: 255 },
                    white: Rgb {
                        r: 255,
                        g: 255,
                        b: 255,
                    },
                },
            },
            mouse: MouseConfig {
                bindings: Vec::new(),
            },
            keyboard: KeyboardConfig {
                bindings: Vec::new(),
            },
        }
    }
}

/// 解析 TOML 文本为 Config（字段级问题入 warnings，不失败）。
///
/// 硬错误（TOML 语法等）返回 Err；调用方（app）回落默认配置并报告。
pub fn parse(text: &str) -> Result<(Config, Warnings), Error> {
    let value: toml::Value = toml::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
    let mut warnings = Warnings::new();
    check_unknown(&value, "", &mut warnings);
    let config = parse_config(&value, &mut warnings);
    Ok((config, warnings))
}

/// 当前二进制名（不硬编码用户路径；args[0] 的 file_stem，空回落 "tty"）。
pub fn app_name() -> String {
    std::env::args()
        .next()
        .and_then(|a| {
            PathBuf::from(a)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "tty".to_string())
}

/// 默认配置文件路径：
/// Linux `$XDG_CONFIG_HOME/<app>/<app>.toml`（未设置则 `$HOME/.config/...`）；
/// Windows `%APPDATA%\<app>\<app>.toml`。
pub fn default_config_path() -> Result<PathBuf, Error> {
    let name = app_name();
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Path("APPDATA 未设置".to_string()))?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| Error::Path("XDG_CONFIG_HOME 与 HOME 均未设置".to_string()))?;
    Ok(base.join(&name).join(format!("{}.toml", name)))
}

/// 配置文件路径优先级：命令行 > APP_CONFIG 环境变量 > 默认路径。
pub fn resolve_config_path(cli: Option<PathBuf>) -> Result<PathBuf, Error> {
    if let Some(p) = cli {
        return Ok(p);
    }
    if let Ok(v) = std::env::var("APP_CONFIG") {
        return Ok(PathBuf::from(v));
    }
    default_config_path()
}

// —— 未知字段检查（带路径警告，不静默忽略）——

/// 已知字段路径（数组元素字段写作 `[]`，如 `mouse.bindings[].mouse`）。
const KNOWN: &[&str] = &[
    "window",
    "window.position",
    "window.position.x",
    "window.position.y",
    "window.dimensions",
    "window.dimensions.columns",
    "window.dimensions.lines",
    "window.padding",
    "window.padding.x",
    "window.padding.y",
    "window.dynamic_title",
    "window.opacity",
    "scrolling",
    "scrolling.history",
    "scrolling.multiplier",
    "scrolling.alt_screen_scroll",
    "terminal",
    "terminal.shell",
    "terminal.shell.program",
    "terminal.shell.args",
    "font",
    "font.size",
    "font.normal",
    "font.normal.family",
    "font.normal.style",
    "font.bold",
    "font.bold.family",
    "font.bold.style",
    "font.italic",
    "font.italic.family",
    "font.italic.style",
    "font.bold_italic",
    "font.bold_italic.family",
    "font.bold_italic.style",
    "font.fallback",
    "font.fallback[].family",
    "font.fallback[].style",
    "font.antialiasing",
    "font.subpixel",
    "font.hinting",
    "font.text_gamma",
    "selection",
    "selection.save_to_clipboard",
    "selection.semantic_escape_chars",
    "colors",
    "colors.draw_bold_text_with_bright_colors",
    "colors.primary",
    "colors.primary.background",
    "colors.primary.foreground",
    "colors.cursor",
    "colors.cursor.text",
    "colors.cursor.cursor",
    "colors.normal",
    "colors.normal.black",
    "colors.normal.red",
    "colors.normal.green",
    "colors.normal.yellow",
    "colors.normal.blue",
    "colors.normal.magenta",
    "colors.normal.cyan",
    "colors.normal.white",
    "colors.bright",
    "colors.bright.black",
    "colors.bright.red",
    "colors.bright.green",
    "colors.bright.yellow",
    "colors.bright.blue",
    "colors.bright.magenta",
    "colors.bright.cyan",
    "colors.bright.white",
    "mouse",
    "mouse.bindings",
    "mouse.bindings[].mouse",
    "mouse.bindings[].mods",
    "mouse.bindings[].action",
    "keyboard",
    "keyboard.bindings",
    "keyboard.bindings[].key",
    "keyboard.bindings[].mods",
    "keyboard.bindings[].action",
];

/// 路径归一化：把 `[n]` 压成 `[]` 后再与 KNOWN 比对。
fn normalize_path(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        if c == '[' {
            out.push_str("[]");
            for c2 in chars.by_ref() {
                if c2 == ']' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_known(path: &str) -> bool {
    KNOWN.contains(&normalize_path(path).as_str())
}

/// 递归检查未知字段，输出带路径警告。
fn check_unknown(v: &toml::Value, prefix: &str, warnings: &mut Warnings) {
    match v {
        toml::Value::Table(t) => {
            for (k, val) in t {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if !is_known(&p) {
                    warnings.push(format!("未知字段 `{p}`，已忽略"));
                }
                check_unknown(val, &p, warnings);
            }
        }
        toml::Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                check_unknown(val, &format!("{prefix}[{i}]"), warnings);
            }
        }
        _ => {}
    }
}

// —— 逐字段解析（字段级问题 → 警告 + 回落默认）——

type Table = toml::value::Table;

fn table<'t>(t: &'t Table, key: &str, path: &str, w: &mut Warnings) -> Option<&'t Table> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_table() {
            Some(tt) => Some(tt),
            None => {
                w.push(format!("{path}.{key}: expected table, ignored"));
                None
            }
        },
    }
}

fn bool_field(t: &Table, key: &str, path: &str, w: &mut Warnings) -> Option<bool> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_bool() {
            Some(b) => Some(b),
            None => {
                w.push(format!("{path}.{key}: expected boolean, ignored"));
                None
            }
        },
    }
}

fn string_field(t: &Table, key: &str, path: &str, w: &mut Warnings) -> Option<String> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_str() {
            Some(s) if !s.is_empty() => Some(s.to_string()),
            _ => {
                w.push(format!("{path}.{key}: expected non-empty string, ignored"));
                None
            }
        },
    }
}

fn u64_field(t: &Table, key: &str, path: &str, min: u64, max: u64, w: &mut Warnings) -> Option<u64> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_integer() {
            Some(n) if n >= min as i64 && n <= max as i64 => Some(n as u64),
            _ => {
                w.push(format!("{path}.{key}: expected integer {min}..={max}, ignored"));
                None
            }
        },
    }
}

fn f32_field(
    t: &Table,
    key: &str,
    path: &str,
    min: f64,
    max: f64,
    w: &mut Warnings,
) -> Option<f32> {
    match t.get(key) {
        None => None,
        Some(v) => {
            let n = match v {
                toml::Value::Float(f) => *f,
                toml::Value::Integer(i) => *i as f64,
                _ => {
                    w.push(format!("{path}.{key}: expected float {min}..={max}, ignored"));
                    return None;
                }
            };
            if n >= min && n <= max {
                Some(n as f32)
            } else {
                w.push(format!("{path}.{key}: expected float {min}..={max}, ignored"));
                None
            }
        }
    }
}

fn color_field(t: &Table, key: &str, path: &str, w: &mut Warnings) -> Option<Rgb> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_str().and_then(|s| s.parse::<Rgb>().ok()) {
            Some(c) => Some(c),
            None => {
                w.push(format!("{path}.{key}: expected color #rrggbb, ignored"));
                None
            }
        },
    }
}

/// 从父表取 key 子表解析 FontSpec。
fn font_spec_field(t: &Table, key: &str, path: &str, w: &mut Warnings) -> Option<FontSpec> {
    match table(t, key, path, w) {
        None => None,
        Some(tt) => font_spec_from_table(tt, &format!("{path}.{key}"), w),
    }
}

/// 解析 FontSpec 表（family + style 必填非空）。
fn font_spec_from_table(tt: &Table, path: &str, w: &mut Warnings) -> Option<FontSpec> {
    let family = match string_field(tt, "family", path, w) {
        Some(f) => f,
        None => {
            w.push(format!("{path}.family: required non-empty string, ignored"));
            return None;
        }
    };
    let style = match string_field(tt, "style", path, w) {
        Some(s) => s,
        None => {
            w.push(format!("{path}.style: required non-empty string, ignored"));
            return None;
        }
    };
    Some(FontSpec { family, style })
}

fn parse_config(v: &toml::Value, w: &mut Warnings) -> Config {
    let mut c = Config::default();
    let Some(root) = v.as_table() else {
        w.push("root: expected table, using defaults".to_string());
        return c;
    };

    if let Some(t) = table(root, "window", "root", w) {
        if let Some(p) = table(t, "position", "window", w) {
            let x = i64_field(p, "x", "window.position", w);
            let y = i64_field(p, "y", "window.position", w);
            if let (Some(x), Some(y)) = (x, y) {
                c.window.position = Some(Pos { x, y });
            }
        }
        if let Some(d) = table(t, "dimensions", "window", w) {
            let cols = u64_field(
                d,
                "columns",
                "window.dimensions",
                1,
                1000,
                w,
            );
            let lines = u64_field(d, "lines", "window.dimensions", 1, 1000, w);
            if let (Some(cols), Some(lines)) = (cols, lines) {
                c.window.dimensions = Dimensions {
                    columns: cols as u16,
                    lines: lines as u16,
                };
            }
        }
        if let Some(p) = table(t, "padding", "window", w) {
            let x = u64_field(p, "x", "window.padding", 0, 1000, w);
            let y = u64_field(p, "y", "window.padding", 0, 1000, w);
            if let (Some(x), Some(y)) = (x, y) {
                c.window.padding = Padding {
                    x: x as u16,
                    y: y as u16,
                };
            }
        }
        if let Some(b) = bool_field(t, "dynamic_title", "window", w) {
            c.window.dynamic_title = b;
        }
        if let Some(o) = f32_field(t, "opacity", "window", 0.0, 1.0, w) {
            c.window.opacity = o;
        }
    }

    if let Some(t) = table(root, "scrolling", "root", w) {
        if let Some(b) = bool_field(t, "alt_screen_scroll", "scrolling", w) {
            c.scrolling.alt_screen_scroll = b;
        }
        if let Some(h) = u64_field(t, "history", "scrolling", 0, 1_000_000, w) {
            c.scrolling.history = h;
        }
        if let Some(m) = f32_field(t, "multiplier", "scrolling", 0.0, 100.0, w) {
            if m > 0.0 {
                c.scrolling.multiplier = m;
            } else {
                w.push("scrolling.multiplier: expected float >0..=100, ignored".to_string());
            }
        }
    }

    if let Some(t) = table(root, "terminal", "root", w)
        && let Some(s) = table(t, "shell", "terminal", w)
    {
        let program = string_field(s, "program", "terminal.shell", w);
        let args = match s.get("args") {
            None => None,
            Some(toml::Value::Array(a)) => Some(
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>(),
            ),
            Some(_) => {
                w.push("terminal.shell.args: expected array, ignored".to_string());
                None
            }
        };
        if let Some(program) = program {
            c.terminal.shell = Some(ShellConfig {
                program,
                args: args.unwrap_or_default(),
            });
        }
    }

    if let Some(t) = table(root, "font", "root", w) {
        if let Some(s) = f32_field(t, "size", "font", 0.0, 100.0, w) {
            if s > 0.0 {
                c.font.size = s;
            } else {
                w.push("font.size: expected float >0..=100, ignored".to_string());
            }
        }
        if let Some(f) = font_spec_field(t, "normal", "font", w) {
            c.font.normal = f;
        }
        if let Some(f) = font_spec_field(t, "bold", "font", w) {
            c.font.bold = f;
        }
        if let Some(f) = font_spec_field(t, "italic", "font", w) {
            c.font.italic = f;
        }
        if let Some(f) = font_spec_field(t, "bold_italic", "font", w) {
            c.font.bold_italic = f;
        }
        if let Some(fb) = parse_fallback(t, w) {
            c.font.fallback = fb;
        }
        if let Some(s) = string_field(t, "antialiasing", "font", w) {
            c.font.antialiasing = match s.as_str() {
                "grayscale" => Antialiasing::Grayscale,
                "none" => Antialiasing::None,
                _ => {
                    w.push("font.antialiasing: expected grayscale|none, ignored".to_string());
                    Antialiasing::Grayscale
                }
            };
        }
        if let Some(s) = string_field(t, "subpixel", "font", w) {
            c.font.subpixel = match s.as_str() {
                "none" => Subpixel::None,
                "rgb" => Subpixel::Rgb,
                "bgr" => Subpixel::Bgr,
                _ => {
                    w.push("font.subpixel: expected none|rgb|bgr, ignored".to_string());
                    Subpixel::None
                }
            };
        }
        if let Some(b) = bool_field(t, "hinting", "font", w) {
            c.font.hinting = b;
        }
        if let Some(g) = f32_field(t, "text_gamma", "font", 0.0, 10.0, w) {
            if g > 0.0 {
                c.font.text_gamma = g;
            } else {
                w.push("font.text_gamma: expected float >0..=10, ignored".to_string());
            }
        }
    }

    if let Some(t) = table(root, "selection", "root", w) {
        if let Some(b) = bool_field(t, "save_to_clipboard", "selection", w) {
            c.selection.save_to_clipboard = b;
        }
        if let Some(s) = string_field(t, "semantic_escape_chars", "selection", w) {
            c.selection.semantic_escape_chars = s;
        }
    }

    if let Some(t) = table(root, "colors", "root", w) {
        if let Some(b) = bool_field(
            t,
            "draw_bold_text_with_bright_colors",
            "colors",
            w,
        ) {
            c.colors.draw_bold_text_with_bright_colors = b;
        }
        if let Some(p) = table(t, "primary", "colors", w) {
            if let Some(bg) = color_field(p, "background", "colors.primary", w) {
                c.colors.primary.background = bg;
            }
            if let Some(fg) = color_field(p, "foreground", "colors.primary", w) {
                c.colors.primary.foreground = fg;
            }
        }
        if let Some(cu) = table(t, "cursor", "colors", w) {
            if let Some(tx) = color_field(cu, "text", "colors.cursor", w) {
                c.colors.cursor.text = tx;
            }
            if let Some(cr) = color_field(cu, "cursor", "colors.cursor", w) {
                c.colors.cursor.cursor = cr;
            }
        }
        if let Some(n) = table(t, "normal", "colors", w) {
            parse_ansi8(n, "colors.normal", &mut c.colors.normal, w);
        }
        if let Some(b) = table(t, "bright", "colors", w) {
            parse_ansi8(b, "colors.bright", &mut c.colors.bright, w);
        }
    }

    if let Some(t) = table(root, "mouse", "root", w) {
        c.mouse.bindings = parse_bindings(t, "mouse", true, w);
    }
    if let Some(t) = table(root, "keyboard", "root", w) {
        c.keyboard.bindings = parse_bindings(t, "keyboard", false, w);
    }

    c
}

fn i64_field(t: &Table, key: &str, path: &str, w: &mut Warnings) -> Option<i64> {
    match t.get(key) {
        None => None,
        Some(v) => match v.as_integer() {
            Some(n) => Some(n),
            None => {
                w.push(format!("{path}.{key}: expected integer, ignored"));
                None
            }
        },
    }
}

fn parse_fallback(t: &Table, w: &mut Warnings) -> Option<Vec<FontSpec>> {
    match t.get("fallback") {
        None => None,
        Some(toml::Value::Array(a)) => {
            let mut out = Vec::new();
            for (i, item) in a.iter().enumerate() {
                match item.as_table() {
                    Some(tt) => {
                        if let Some(fs) =
                            font_spec_from_table(tt, &format!("font.fallback[{i}]"), w)
                        {
                            out.push(fs);
                        }
                    }
                    None => w.push(format!("font.fallback[{i}]: expected table, ignored")),
                }
            }
            Some(out)
        }
        Some(_) => {
            w.push("font.fallback: expected array, ignored".to_string());
            None
        }
    }
}

fn parse_ansi8(t: &Table, path: &str, dst: &mut Ansi8, w: &mut Warnings) {
    if let Some(c) = color_field(t, "black", path, w) {
        dst.black = c;
    }
    if let Some(c) = color_field(t, "red", path, w) {
        dst.red = c;
    }
    if let Some(c) = color_field(t, "green", path, w) {
        dst.green = c;
    }
    if let Some(c) = color_field(t, "yellow", path, w) {
        dst.yellow = c;
    }
    if let Some(c) = color_field(t, "blue", path, w) {
        dst.blue = c;
    }
    if let Some(c) = color_field(t, "magenta", path, w) {
        dst.magenta = c;
    }
    if let Some(c) = color_field(t, "cyan", path, w) {
        dst.cyan = c;
    }
    if let Some(c) = color_field(t, "white", path, w) {
        dst.white = c;
    }
}

fn parse_bindings(t: &Table, path: &str, expect_mouse: bool, w: &mut Warnings) -> Vec<Binding> {
    let mut out = Vec::new();
    let items = match t.get("bindings") {
        None => return out,
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            w.push(format!("{path}.bindings: expected array, ignored"));
            return out;
        }
    };
    for (i, item) in items.iter().enumerate() {
        let item_path = format!("{path}.bindings[{i}]");
        let tt = match item.as_table() {
            Some(tt) => tt,
            None => {
                w.push(format!("{item_path}: expected table, ignored"));
                continue;
            }
        };
        let action = match tt.get("action").and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
        {
            Some(a) => a,
            _ => {
                w.push(format!("{item_path}.action: required valid action, binding skipped"));
                continue;
            }
        };
        let mouse = if expect_mouse {
            match tt
                .get("mouse")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
            {
                Some(m) => Some(m),
                None => {
                    w.push(format!("{item_path}.mouse: required valid mouse button, binding skipped"));
                    continue;
                }
            }
        } else {
            None
        };
        let key = if !expect_mouse {
            match tt.get("key").and_then(|v| v.as_str()) {
                Some(k) if !k.is_empty() => Some(k.to_string()),
                _ => {
                    w.push(format!("{item_path}.key: required non-empty key, binding skipped"));
                    continue;
                }
            }
        } else {
            None
        };
        let mods = match tt.get("mods") {
            None => Mods::default(),
            Some(v) => match v.as_str().and_then(|s| s.parse().ok()) {
                Some(m) => m,
                None => {
                    w.push(format!("{item_path}.mods: invalid mods, using empty"));
                    Mods::default()
                }
            },
        };
        out.push(Binding {
            mouse,
            key,
            mods,
            action,
        });
    }
    out
}
