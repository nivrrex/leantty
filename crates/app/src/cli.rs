//! 命令行参数解析（阶段 5A t2）。
//!
//! 只用 `std::env::args_os`（路径按 `OsString` 处理，非 UTF-8 路径不会被破坏），
//! 解析逻辑为**无 I/O 纯函数**，便于表驱动测试。帮助/版本文本与退出码由调用方决定：
//! 解析发生在创建事件循环与任何显示连接之前，因此 `--help`/`--version` 在无 `DISPLAY`
//! 的环境下也能工作。

use std::ffi::OsString;

/// 解析后的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    /// `-h` / `--help`：打印用法后退出 0。
    Help,
    /// `-V` / `--version`：打印版本后退出 0。
    Version,
    /// 正常运行；`config_file` 为显式指定的配置文件（`None` 表示走环境变量/默认路径）。
    Run {
        config_file: Option<OsString>,
        /// `-e/--command` 之后的 argv（程序 + 参数）；非 UTF-8 已在解析期拒绝。
        command: Option<Vec<String>>,
        /// `--working-directory` 的值（已是 OsString，路径可能非 UTF-8）。
        working_directory: Option<OsString>,
    },
}

/// 解析错误。调用方负责打印并以退出码 2 结束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// 未知选项（以 `-` 开头但不认识）。
    UnknownOption(String),
    /// 需要值的选项后面没有值。
    MissingValue(&'static str),
    /// 多余的位置参数。
    UnexpectedArgument(String),
    /// 同一选项重复出现（`--working-directory`）。
    RepeatedOption(&'static str),
    /// `-e/--command` 的参数不是合法 UTF-8。
    NonUtf8CommandArg,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::UnknownOption(s) => write!(f, "unknown option '{}'", s),
            CliError::MissingValue(name) => write!(f, "option '{}' requires a value", name),
            CliError::UnexpectedArgument(s) => write!(f, "unexpected argument '{}'", s),
            CliError::RepeatedOption(name) => write!(f, "option '{}' may only be given once", name),
            CliError::NonUtf8CommandArg => write!(
                f,
                "command arguments after -e/--command must be valid UTF-8"
            ),
        }
    }
}

/// 配置文件的显式选项（`--config` 是 `--config-file` 的隐藏别名，保留给既有 Windows 脚本）。
const CONFIG_FLAGS: [&str; 2] = ["--config-file", "--config"];
const CONFIG_PREFIXES: [&[u8]; 2] = [b"--config-file=", b"--config="];

/// 解析除程序名之外的参数。
///
/// 规则：
/// - `-h`/`--help`、`-V`/`--version` 一旦出现立即生效（先出现者优先，其余参数不再校验）；
/// - `--config-file <PATH>` 与 `--config-file=<PATH>` 等价（`--config` 为隐藏别名）；
/// - 其它以 `-` 开头的参数 → `UnknownOption`；不以 `-` 开头的 → `UnexpectedArgument`；
/// - 选项缺值（含 `--config-file=` 空值）→ `MissingValue`。
pub fn parse_args<I>(args: I) -> Result<Cli, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut it = args.into_iter();
    let mut config_file: Option<OsString> = None;
    let mut working_directory: Option<OsString> = None;
    let mut command: Option<Vec<String>> = None;
    while let Some(arg) = it.next() {
        let text = arg.to_str();
        match text {
            Some("-h") | Some("--help") => return Ok(Cli::Help),
            Some("-V") | Some("--version") => return Ok(Cli::Version),
            _ => {}
        }
        if CONFIG_FLAGS.iter().any(|f| text == Some(*f)) {
            match it.next() {
                Some(v) => config_file = Some(v),
                None => return Err(CliError::MissingValue("--config-file")),
            }
            continue;
        }
        // `--config-file=<路径>`：在 OsStr 字节层面切分，避免非 UTF-8 路径被破坏。
        let bytes = arg.as_encoded_bytes();
        if let Some(prefix) = CONFIG_PREFIXES.iter().find(|p| bytes.starts_with(p)) {
            let value = &bytes[prefix.len()..];
            if value.is_empty() {
                return Err(CliError::MissingValue("--config-file"));
            }
            // SAFETY: 只在这个前缀之后切分，前缀本身是合法的 UTF-8 字节序列，
            // 因此尾部仍是原 `OsString` 的合法编码，可无损还原。
            let value = unsafe { OsString::from_encoded_bytes_unchecked(value.to_vec()) };
            config_file = Some(value);
            continue;
        }
        // --working-directory <DIR> / --working-directory=<DIR>（重复出现报错，避免歧义）
        if text == Some("--working-directory") {
            if working_directory.is_some() {
                return Err(CliError::RepeatedOption("--working-directory"));
            }
            match it.next() {
                Some(v) => working_directory = Some(v),
                None => return Err(CliError::MissingValue("--working-directory")),
            }
            continue;
        }
        if let Some(prefix) = bytes.strip_prefix(b"--working-directory=") {
            if working_directory.is_some() {
                return Err(CliError::RepeatedOption("--working-directory"));
            }
            if prefix.is_empty() {
                return Err(CliError::MissingValue("--working-directory"));
            }
            // SAFETY: 前缀是合法 UTF-8 字节序列，尾部仍是原 OsString 的合法编码。
            working_directory =
                Some(unsafe { OsString::from_encoded_bytes_unchecked(prefix.to_vec()) });
            continue;
        }
        // -e / --command：其后全部参数（含以 - 开头者）作为子进程 argv，停止解析选项
        if text == Some("-e") || text == Some("--command") {
            let mut argv = Vec::new();
            for a in it.by_ref() {
                match a.into_string() {
                    Ok(v) => argv.push(v),
                    Err(_) => return Err(CliError::NonUtf8CommandArg),
                }
            }
            if argv.is_empty() {
                return Err(CliError::MissingValue("-e"));
            }
            command = Some(argv);
            break;
        }
        if bytes.first() == Some(&b'-') {
            return Err(CliError::UnknownOption(
                text.unwrap_or("<非 UTF-8 选项>").to_string(),
            ));
        }
        return Err(CliError::UnexpectedArgument(
            text.unwrap_or("<非 UTF-8 参数>").to_string(),
        ));
    }
    Ok(Cli::Run {
        config_file,
        command,
        working_directory,
    })
}

/// 帮助文本：只含用法、三个选项、`LEANTTY_CONFIG` 一行与配置默认路径。
pub fn help_text(default_config_path: &str) -> String {
    format!(
        "Usage: leantty [OPTIONS]\n\
         \n\
         Options:\n\
         \x20 -h, --help               Print this help and exit\n\
         \x20 -V, --version            Print version and exit\n\
         \x20     --config-file <PATH> Use the given configuration file\n\
         \x20     --working-directory <DIR>\n\
         \x20                         Start in this directory (the program inherits it)\n\
         \x20 -e, --command <PROGRAM> [ARGS...]\n\
         \x20                         Run PROGRAM instead of the configured shell; every\n\
         \x20                         following argument is passed to it verbatim\n\
         \n\
         Environment:\n\
         \x20 LEANTTY_CONFIG            Configuration file path (used when --config-file is absent)\n\
         \n\
         Default configuration file: {}\n",
        default_config_path
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cli, CliError> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn no_args_runs_with_defaults() {
        assert_eq!(
            p(&[]),
            Ok(Cli::Run {
                config_file: None,
                command: None,
                working_directory: None
            })
        );
    }

    #[test]
    fn command_stops_option_parsing() {
        let got = parse_args(
            ["-e", "sh", "-c", "echo hi", "--config-file"]
                .iter()
                .map(OsString::from),
        );
        assert_eq!(
            got,
            Ok(Cli::Run {
                config_file: None,
                command: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "echo hi".to_string(),
                    "--config-file".to_string()
                ]),
                working_directory: None,
            }),
            "-e 之后全部参数（含以 - 开头者）都是子进程 argv"
        );
        // -e 之前的选项正常解析
        let got = parse_args(
            ["--config-file", "a.toml", "-e", "htop"]
                .iter()
                .map(OsString::from),
        );
        assert_eq!(
            got,
            Ok(Cli::Run {
                config_file: Some("a.toml".into()),
                command: Some(vec!["htop".to_string()]),
                working_directory: None,
            })
        );
        // -e 缺程序名
        assert_eq!(
            parse_args(["-e"].iter().map(OsString::from)),
            Err(CliError::MissingValue("-e"))
        );
        assert_eq!(
            parse_args(["--command"].iter().map(OsString::from)),
            Err(CliError::MissingValue("-e"))
        );
    }

    #[test]
    fn working_directory_forms_and_errors() {
        let want = |v: &str| {
            Ok(Cli::Run {
                config_file: None,
                command: None,
                working_directory: Some(OsString::from(v)),
            })
        };
        assert_eq!(
            parse_args(["--working-directory", "/x"].iter().map(OsString::from)),
            want("/x")
        );
        assert_eq!(
            parse_args(["--working-directory=/x"].iter().map(OsString::from)),
            want("/x")
        );
        assert_eq!(
            parse_args(["--working-directory"].iter().map(OsString::from)),
            Err(CliError::MissingValue("--working-directory"))
        );
        assert_eq!(
            parse_args(["--working-directory="].iter().map(OsString::from)),
            Err(CliError::MissingValue("--working-directory"))
        );
        assert_eq!(
            parse_args(
                ["--working-directory", "/a", "--working-directory", "/b"]
                    .iter()
                    .map(OsString::from)
            ),
            Err(CliError::RepeatedOption("--working-directory")),
            "重复给出 --working-directory 报错（DECISIONS 记录）"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_command_arg_is_rejected() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0xff]);
        assert_eq!(
            parse_args([OsString::from("-e"), OsString::from("sh"), bad]),
            Err(CliError::NonUtf8CommandArg)
        );
    }

    #[test]
    fn help_and_version_variants() {
        assert_eq!(p(&["-h"]), Ok(Cli::Help));
        assert_eq!(p(&["--help"]), Ok(Cli::Help));
        assert_eq!(p(&["-V"]), Ok(Cli::Version));
        assert_eq!(p(&["--version"]), Ok(Cli::Version));
        // 先出现者优先，且不再校验其余参数
        assert_eq!(p(&["--help", "--bogus"]), Ok(Cli::Help));
        assert_eq!(
            p(&["--bogus", "--help"]),
            Err(CliError::UnknownOption("--bogus".into()))
        );
    }

    #[test]
    fn config_file_with_space_and_equals() {
        assert_eq!(
            p(&["--config-file", "a.toml"]),
            Ok(Cli::Run {
                config_file: Some("a.toml".into()),
                command: None,
                working_directory: None,
            })
        );
        assert_eq!(
            p(&["--config-file=a.toml"]),
            Ok(Cli::Run {
                config_file: Some("a.toml".into()),
                command: None,
                working_directory: None,
            })
        );
        // 路径含空格/等号时按"第一段等号之后整体"取值
        assert_eq!(
            p(&["--config-file=/tmp/a b=c.toml"]),
            Ok(Cli::Run {
                config_file: Some("/tmp/a b=c.toml".into()),
                command: None,
                working_directory: None,
            })
        );
        // 隐藏别名（既有 Windows 脚本用）
        assert_eq!(
            p(&["--config", "w.toml"]),
            Ok(Cli::Run {
                config_file: Some("w.toml".into()),
                command: None,
                working_directory: None,
            })
        );
    }

    #[test]
    fn config_file_missing_value() {
        assert_eq!(
            p(&["--config-file"]),
            Err(CliError::MissingValue("--config-file"))
        );
        assert_eq!(
            p(&["--config-file="]),
            Err(CliError::MissingValue("--config-file"))
        );
    }

    #[test]
    fn unknown_options_and_positionals() {
        assert_eq!(
            p(&["--nope"]),
            Err(CliError::UnknownOption("--nope".into()))
        );
        assert_eq!(p(&["-x"]), Err(CliError::UnknownOption("-x".into())));
        assert_eq!(
            p(&["foo.toml"]),
            Err(CliError::UnexpectedArgument("foo.toml".into()))
        );
    }

    #[test]
    fn error_messages_are_single_line() {
        for e in [
            CliError::UnknownOption("--x".into()),
            CliError::MissingValue("--config-file"),
            CliError::UnexpectedArgument("p".into()),
        ] {
            let s = e.to_string();
            assert!(!s.contains('\n'), "错误文本必须是一行: {s:?}");
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn help_text_mentions_only_documented_items() {
        let t = help_text("/home/u/.config/leantty/leantty.toml");
        for needle in [
            "Usage: leantty",
            "-h, --help",
            "-V, --version",
            "--config-file <PATH>",
            "LEANTTY_CONFIG",
            "Default configuration file: /home/u/.config/leantty/leantty.toml",
        ] {
            assert!(t.contains(needle), "帮助文本缺少 {needle:?}:\n{t}");
        }
        // 不得出现已废弃的 --font / --font-size 选项
        assert!(!t.contains("--font"));
    }

    #[test]
    fn non_utf8_path_is_preserved() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = vec![b'/', b'a', 0xff, b'.'];
            let arg = OsString::from_vec(raw.clone());
            let got = parse_args([OsString::from("--config-file"), arg]);
            match got {
                Ok(Cli::Run {
                    config_file: Some(f),
                    ..
                }) => {
                    use std::os::unix::ffi::OsStrExt;
                    assert_eq!(f.as_os_str().as_bytes(), raw.as_slice());
                }
                other => panic!("应保留非 UTF-8 路径，实测 {other:?}"),
            }
        }
    }
}
