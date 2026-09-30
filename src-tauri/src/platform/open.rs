//! 用系统默认方式打开链接或文件夹。
//!
//! 只给 Rust 侧固定入口（Release 页面、日志目录）使用，前端不能传入任意目标。
//! macOS 用 `open`，Windows 用 `explorer`（文件夹）与 `rundll32 url.dll,FileProtocolHandler`（链接）：
//! Windows 分支来自系统命令惯例，未实机验证。

use std::path::Path;
use std::process::Command;

fn spawn(mut command: Command) -> bool {
    command.spawn().is_ok()
}

pub fn open_url(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        spawn({
            let mut command = Command::new("open");
            command.arg(url);
            command
        })
    }
    #[cfg(target_os = "windows")]
    {
        spawn({
            let mut command = Command::new("rundll32");
            command.arg("url.dll,FileProtocolHandler").arg(url);
            command
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        spawn({
            let mut command = Command::new("xdg-open");
            command.arg(url);
            command
        })
    }
}

pub fn open_folder(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        spawn({
            let mut command = Command::new("explorer");
            command.arg(path);
            command
        })
    }
    #[cfg(target_os = "macos")]
    {
        spawn({
            let mut command = Command::new("open");
            command.arg(path);
            command
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        spawn({
            let mut command = Command::new("xdg-open");
            command.arg(path);
            command
        })
    }
}
