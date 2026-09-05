//! Guest-facing `ui.*` ABI. This module contains no windowing lifecycle code.

use wasmtime::{Caller, Result};

use crate::host::{Host, UiCommand, shared_mem};

const MAX_TEXT_BYTES: i32 = 65_536;

fn text(caller: &mut Caller<'_, Host>, ptr: i32, len: i32) -> Result<String> {
    if ptr < 0 || len < 0 || len > MAX_TEXT_BYTES {
        return Err(wasmtime::Error::msg("ui text pointer or length is invalid"));
    }
    let memory = shared_mem(caller)?;
    let mut bytes = vec![0; len as usize];
    memory.read(&*caller, ptr as usize, &mut bytes)?;
    String::from_utf8(bytes).map_err(|_| wasmtime::Error::msg("ui text must be UTF-8"))
}

pub fn label(mut caller: Caller<'_, Host>, ptr: i32, len: i32) -> Result<()> {
    let value = text(&mut caller, ptr, len)?;
    caller.data_mut().ui.push(UiCommand::Label(value));
    Ok(())
}

pub fn button(mut caller: Caller<'_, Host>, ptr: i32, len: i32) -> Result<i32> {
    let value = text(&mut caller, ptr, len)?;
    let clicked = caller.data().ui_clicked.contains(&value);
    caller.data_mut().ui.push(UiCommand::Button(value));
    Ok(clicked as i32)
}
