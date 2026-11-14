//! Minimal Discord local RPC client (named pipe `discord-ipc-N`).

use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};

pub struct Discord {
    pipe: File,
    nonce: u64,
}

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

impl Discord {
    /// Tries every Discord pipe; `Err` carries a message worth logging
    /// (for example a rejected Application ID), `Ok(None)` means Discord isn't running.
    pub fn connect(client_id: &str) -> Result<Option<Discord>, String> {
        let mut last_err = None;
        for i in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{i}");
            let Ok(pipe) = OpenOptions::new().read(true).write(true).open(&path) else { continue };
            let mut d = Discord { pipe, nonce: 0 };
            let hello = json!({ "v": 1, "client_id": client_id }).to_string();
            match d.send(OP_HANDSHAKE, &hello).and_then(|_| d.read()) {
                Ok((OP_CLOSE, text)) => last_err = Some(format!("Discord rejected the Application ID: {text}")),
                Ok(_) => return Ok(Some(d)),
                Err(e) => last_err = Some(format!("Discord handshake failed: {e}")),
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }

    fn send(&mut self, op: u32, json: &str) -> io::Result<()> {
        let body = json.as_bytes();
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(body);
        self.pipe.write_all(&frame)?;
        self.pipe.flush()
    }

    fn read(&mut self) -> io::Result<(u32, String)> {
        let mut head = [0u8; 8];
        self.pipe.read_exact(&mut head)?;
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len > 1 << 20 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized frame"));
        }
        let mut body = vec![0u8; len];
        self.pipe.read_exact(&mut body)?;
        Ok((op, String::from_utf8_lossy(&body).into_owned()))
    }

    /// Sets (or with `Value::Null` clears) the activity. `Ok(Some(msg))` means
    /// Discord answered but refused the update.
    pub fn set_activity(&mut self, activity: Value) -> io::Result<Option<String>> {
        self.nonce += 1;
        let nonce = format!("amrpc-{}", self.nonce);
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "nonce": nonce,
            "args": { "pid": std::process::id(), "activity": activity },
        });
        self.send(OP_FRAME, &payload.to_string())?;
        for _ in 0..5 {
            let (op, text) = self.read()?;
            match op {
                OP_PING => self.send(OP_PONG, &text)?,
                OP_CLOSE => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, text)),
                _ => {
                    let msg: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if msg["nonce"].as_str() == Some(nonce.as_str()) {
                        if msg["evt"].as_str() == Some("ERROR") {
                            return Ok(Some(msg["data"]["message"].as_str().unwrap_or("unknown error").to_string()));
                        }
                        return Ok(None);
                    }
                }
            }
        }
        Ok(None)
    }
}
