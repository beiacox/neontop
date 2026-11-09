//! Instancia única. La primera instancia toma un cerrojo (`flock`) sobre `neontop.lock` y escucha
//! en el socket `neontop.sock` de $XDG_RUNTIME_DIR; las siguientes no consiguen el cerrojo, piden a
//! la primera que muestre su ventana y terminan. El cerrojo es atómico: dos arranques en el mismo
//! instante (dos entradas de autoarranque, por ejemplo) no pueden quedarse ambos.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;

use crate::state::Msg;

/// El cerrojo de la instancia principal: se libera solo al terminar el proceso.
static LOCK: OnceLock<File> = OnceLock::new();

fn path() -> PathBuf {
    // NEONTOP_SOCKET: otra ruta (pruebas con una segunda instancia aislada)
    std::env::var_os("NEONTOP_SOCKET").map(PathBuf::from).unwrap_or_else(|| neontop_core::paths::runtime_dir().join("neontop.sock"))
}

fn lock_path(sock: &Path) -> PathBuf {
    sock.with_extension("lock")
}

/// Intenta ser la instancia principal: `Some(cerrojo)` si lo es.
fn try_claim(sock: &Path) -> Option<File> {
    let f = OpenOptions::new().create(true).truncate(false).write(true).open(lock_path(sock)).ok()?;
    match f.try_lock() {
        Ok(()) => Some(f),
        Err(std::fs::TryLockError::WouldBlock) => None,
        // sin soporte de cerrojos (sistema de ficheros raro): mejor arrancar que no hacerlo
        Err(std::fs::TryLockError::Error(_)) => Some(f),
    }
}

/// Pide a la instancia principal que se muestre; espera a que su socket esté listo (puede estar
/// arrancando a la vez que esta).
fn notify(sock: &Path, wait: Duration) -> bool {
    let start = Instant::now();
    loop {
        if let Ok(mut s) = UnixStream::connect(sock) {
            return s.write_all(b"show\n").is_ok();
        }
        if start.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `true` si esta es la instancia principal. Si no, avisa a la principal para que muestre su
/// ventana y devuelve `false` (esta debe terminar).
pub fn claim() -> bool {
    let sock = path();
    match try_claim(&sock) {
        Some(f) => {
            let _ = LOCK.set(f);
            true
        }
        None => {
            notify(&sock, Duration::from_secs(15));
            false
        }
    }
}

/// Escucha a las instancias siguientes (solo la principal, que tiene el cerrojo).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_sock(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neontop-single-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("neontop.sock")
    }

    #[test]
    fn only_one_claim_wins_even_at_the_same_time() {
        let sock = tmp_sock("race");
        // 8 arranques simultáneos: exactamente uno se queda el cerrojo
        let handles: Vec<_> = (0..8).map(|_| {
            let s = sock.clone();
            std::thread::spawn(move || try_claim(&s))
        }).collect();
        let locks: Vec<File> = handles.into_iter().filter_map(|h| h.join().unwrap()).collect();
        assert_eq!(locks.len(), 1);
        assert!(try_claim(&sock).is_none(), "con el cerrojo tomado nadie más entra");
        drop(locks);
        assert!(try_claim(&sock).is_some(), "al terminar la principal, la siguiente puede serlo");
    }

    #[test]
    fn late_listener_still_gets_the_show_request() {
        let sock = tmp_sock("late");
        let _ = std::fs::remove_file(&sock);
        let s2 = sock.clone();
        // la principal empieza a escuchar 300 ms después de que la segunda intente avisar
        let server = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let l = UnixListener::bind(&s2).unwrap();
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 16];
            let n = c.read(&mut buf).unwrap();
            buf[..n].starts_with(b"show")
        });
        assert!(notify(&sock, Duration::from_secs(5)));
        assert!(server.join().unwrap());
        assert!(!notify(&tmp_sock("nadie"), Duration::from_millis(200)), "sin principal se rinde");
    }
}
