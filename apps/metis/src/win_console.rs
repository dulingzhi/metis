//! 把 CLI 进程关联到父控制台（Windows）。
//!
//! 单 exe 是 **GUI 子系统**：双击不会弹控制台。但 CLI 模式从终端启动时，进程默认
//! **不关联**父控制台；它再启动 console 子进程（claude / codex 等 TUI）时，Windows
//! 会给子进程**新建一个控制台窗口**（用户报过"接管命令另开一个窗口，应该在当前控制台打开"）。
//! `AttachConsole(ATTACH_PARENT_PROCESS)` 把当前进程关联到父控制台，子进程就会继承
//! 同一个控制台（ConPTY 下 `GetConsoleWindow` 仍可能是 0，但不会再新建窗口）。
//!
//! **注意**：`AttachConsole` 会把本进程的 std 句柄切到控制台，导致
//! `metis status > file` / 管道捕获变空（实测）。所以 attach 前保存三个 std 句柄，
//! attach 后立刻用 `SetStdHandle` 还原；控制台关联保持不变。
//!
//! **这是全仓库唯一的 unsafe 例外**（AGENTS.md 铁律 9）：只调用三个 Win32 函数，
//! 无指针、无内存契约。除本文件外任何地方不得出现 `unsafe`。

#![allow(unsafe_code)]

/// `ATTACH_PARENT_PROCESS`：关联到父进程的控制台。
const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
const STD_INPUT_HANDLE: i32 = -10;
const STD_OUTPUT_HANDLE: i32 = -11;
const STD_ERROR_HANDLE: i32 = -12;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
    fn GetStdHandle(kind: i32) -> isize;
    fn SetStdHandle(kind: i32, handle: isize) -> i32;
}

/// 关联到父进程的控制台，并保留调用方的 std 句柄（重定向/管道不被吞掉）。
///
/// 父进程没有控制台（从资源管理器双击、CI 管道）时返回 `false`；
/// 已经有关联时也会失败，同样返回 `false`。失败无副作用，调用方忽略即可。
pub fn attach_parent_console() -> bool {
    // SAFETY: 三个调用都只传枚举常量/句柄，不涉及内存访问；句柄按值保存与还原。
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        let error = GetStdHandle(STD_ERROR_HANDLE);

        let attached = AttachConsole(ATTACH_PARENT_PROCESS) != 0;

        // AttachConsole 可能把 std 句柄切到控制台；还原调用方给我们的句柄。
        if output != 0 {
            SetStdHandle(STD_OUTPUT_HANDLE, output);
        }
        if error != 0 {
            SetStdHandle(STD_ERROR_HANDLE, error);
        }
        if input != 0 {
            SetStdHandle(STD_INPUT_HANDLE, input);
        }
        attached
    }
}
