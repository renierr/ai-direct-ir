//! `net.*` syscalls: the TCP layer WASI preview1 deliberately lacks.
//! Thin wrappers over std::net; all bytes flow through the shared memory.

use std::io::{Read, Write};
use std::net::TcpListener;

use wasmtime::{Caller, Result};

use crate::host::{Host, Sock, shared_mem};

pub fn w_listen(mut caller: Caller<'_, Host>, port: i32) -> Result<i32> {
    let l = match TcpListener::bind(("127.0.0.1", port as u16)) {
        Ok(l) => l,
        Err(_) => return Ok(-1),
    };
    Ok(caller.data_mut().alloc_sock(Sock::Listen(l)))
}

pub fn w_accept(mut caller: Caller<'_, Host>, h: i32) -> Result<i32> {
    let conn = match caller.data().socks.get(&h) {
        Some(Sock::Listen(l)) => match l.accept() {
            Ok((c, _)) => c,
            Err(_) => return Ok(-1),
        },
        _ => return Ok(-1),
    };
    Ok(caller.data_mut().alloc_sock(Sock::Conn(conn)))
}

pub fn w_recv(mut caller: Caller<'_, Host>, h: i32, ptr: i32, len: i32) -> Result<i32> {
    let mut buf = vec![0u8; (len as usize).min(65536)];
    let n = match caller.data_mut().socks.get_mut(&h) {
        Some(Sock::Conn(c)) => match c.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return Ok(-1),
        },
        _ => return Ok(-1),
    };
    if n == 0 {
        return Ok(0);
    }
    shared_mem(&mut caller)?.write(&mut caller, ptr as usize, &buf[..n])?;
    Ok(n as i32)
}

pub fn w_send(mut caller: Caller<'_, Host>, h: i32, ptr: i32, len: i32) -> Result<i32> {
    let mut buf = vec![0u8; len as usize];
    shared_mem(&mut caller)?.read(&caller, ptr as usize, &mut buf)?;
    match caller.data_mut().socks.get_mut(&h) {
        Some(Sock::Conn(c)) => match c.write(&buf) {
            Ok(n) => Ok(n as i32),
            Err(_) => Ok(-1),
        },
        _ => Ok(-1),
    }
}

pub fn w_close(mut caller: Caller<'_, Host>, h: i32) -> Result<i32> {
    caller.data_mut().socks.remove(&h);
    Ok(0)
}
