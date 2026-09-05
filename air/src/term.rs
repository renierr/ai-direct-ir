//! The terminal capability, offered to components as `ai-direct:host/term`.
//!
//! WASI has byte streams but no raw-mode, cursor, size, or key-event API.
//! This module supplies that through crossterm. It is deliberately separate
//! from WASI: a batch command receives no terminal semantics unless it
//! imports the interface.
//!
//! Everything here is a plain function over a `bool`. The WIT shapes -- which
//! calls answer `bool`, which answer nothing -- live in `component.rs`, next
//! to the linker that declares them.

use std::io::{IsTerminal, Write, stdin, stdout};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use wasmtime::Result;

/// Key values returned by `term.read_key`. Printable ASCII returns itself.
pub const KEY_UP: i32 = 0x101;
pub const KEY_DOWN: i32 = 0x102;
pub const KEY_LEFT: i32 = 0x103;
pub const KEY_RIGHT: i32 = 0x104;
pub const KEY_ENTER: i32 = 0x10d;
pub const KEY_ESCAPE: i32 = 0x11b;
pub const KEY_BACKSPACE: i32 = 0x108;
pub const KEY_TAB: i32 = 0x109;
pub const KEY_CTRL_C: i32 = 0x003;

/// Restore a terminal changed by the guest. Safe to call more than once, and
/// called on the way out however the guest left -- a trap, a Ctrl-C, or a
/// clean exit must never strand the user in raw mode.
pub fn restore_flag(active: &mut bool) {
    if !*active {
        return;
    }
    let _ = execute!(stdout(), Show, LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    *active = false;
}

pub fn enter(active: &mut bool) -> i32 {
    if *active {
        return 0;
    }
    if terminal::enable_raw_mode().is_err() {
        return -1;
    }
    if execute!(stdout(), EnterAlternateScreen, Hide, Clear(ClearType::All)).is_err() {
        let _ = terminal::disable_raw_mode();
        return -1;
    }
    *active = true;
    0
}

/// 1 only when both streams are real terminals. Guests use this to retain a
/// scriptable fallback for pipes, redirected files, and CI.
pub fn available() -> i32 {
    i32::from(stdin().is_terminal() && stdout().is_terminal())
}

pub fn exit(active: &mut bool) -> i32 {
    restore_flag(active);
    0
}

pub fn clear(active: bool) -> i32 {
    if !active {
        return -1;
    }
    execute!(stdout(), MoveTo(0, 0), Clear(ClearType::All)).map_or(-1, |_| 0)
}

pub fn move_to(active: bool, x: i32, y: i32) -> i32 {
    if !active || x < 0 || y < 0 || x > u16::MAX as i32 || y > u16::MAX as i32 {
        return -1;
    }
    execute!(stdout(), MoveTo(x as u16, y as u16)).map_or(-1, |_| 0)
}

/// Packed terminal size: columns in high 16 bits, rows in low 16 bits; -1 error.
pub fn size() -> i32 {
    terminal::size().map_or(-1, |(cols, rows)| ((cols as i32) << 16) | rows as i32)
}

pub fn flush() -> i32 {
    stdout().flush().map_or(-1, |_| 0)
}

pub fn read_key(active: bool) -> Result<i32> {
    if !active {
        return Ok(-1);
    }
    loop {
        let event =
            event::read().map_err(|e| wasmtime::Error::msg(format!("terminal read: {e}")))?;
        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let code = match key.code {
            KeyCode::Up => KEY_UP,
            KeyCode::Down => KEY_DOWN,
            KeyCode::Left => KEY_LEFT,
            KeyCode::Right => KEY_RIGHT,
            KeyCode::Enter => KEY_ENTER,
            KeyCode::Esc => KEY_ESCAPE,
            KeyCode::Backspace => KEY_BACKSPACE,
            KeyCode::Tab | KeyCode::BackTab => KEY_TAB,
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                KEY_CTRL_C
            }
            KeyCode::Char(c) if c.is_ascii() => c as i32,
            _ => continue,
        };
        return Ok(code);
    }
}
