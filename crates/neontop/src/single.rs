//! Instancia única: la primera escucha en un socket de $XDG_RUNTIME_DIR; las siguientes le piden
//! que muestre su ventana y terminan.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use futures::channel::mpsc::UnboundedSender;

use crate::state::Msg;

fn path() -> PathBuf {
    neontop_core::paths::runtime_dir().join("neontop.sock")
}

/// Si ya hay una instancia, le pide mostrarse y devuelve `true` (esta debe terminar).
pub fn notify_existing() -> bool {
    match UnixStream::connect(path()) {
        Ok(mut s) => {
            let _ = s.write_all(b"show\n");
            true
        }
        Err(_) => false,
    }
}

/// Escucha a las instancias siguientes.
pub fn listen(tx: UnboundedSender<Msg>) {
    let p = path();
    // socket de una ejecución anterior que terminó sin borrarlo
    let _ = std::fs::remove_file(&p);
    let Ok(l) = UnixListener::bind(&p) else { return };
    std::thread::Builder::new()
        .name("neontop-single".into())
        .spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = [0u8; 16];
                if s.read(&mut buf).is_ok_and(|n| buf[..n].starts_with(b"show")) {
                    let _ = tx.unbounded_send(Msg::Show);
                }
            }
        })
        .ok();
}

pub fn cleanup() {
    let _ = std::fs::remove_file(path());
}
