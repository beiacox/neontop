//! El lector de juegos de NeonTop entiende lo que escribe la capa de FPS (mismo formato).

use neontop_core::collectors::games::GamesCollector;
use neontop_fps_shm::{frame, State, API_GLX};

#[test]
fn collector_reads_frames_written_by_the_layer() {
    let st = State::new();
    let mut col = GamesCollector::new();
    // el primer fotograma crea el segmento
    for _ in 0..10 {
        frame(&st, API_GLX);
    }
    let me = std::process::id();
    let snap = col.sample();
    let g = snap.games.iter().find(|g| g.pid == me).expect("el colector ve el juego de este proceso");
    assert_eq!(g.api, "opengl");
    assert_eq!(g.bits, 64);
    assert_eq!(g.frames, 10);
    assert!(!g.exe.is_empty());
    // al abrir el segmento empieza desde el recuento actual (un juego ya en marcha)
    assert!(g.frametimes.is_empty());
    // fotogramas nuevos (~250 FPS): la siguiente muestra trae sus tiempos
    for _ in 0..50 {
        frame(&st, API_GLX);
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    let g = col.sample().games.into_iter().find(|g| g.pid == me).unwrap();
    assert!(g.frametimes.len() >= 49, "{}", g.frametimes.len());
    let avg = g.frametimes.iter().sum::<f32>() / g.frametimes.len() as f32;
    assert!((3.5..20.0).contains(&avg), "tiempo medio {avg} ms");
    // la siguiente muestra solo trae lo nuevo
    frame(&st, API_GLX);
    let g2 = col.sample().games.into_iter().find(|g| g.pid == me).unwrap();
    assert_eq!(g2.frametimes.len(), 1);
    let _ = std::fs::remove_file(format!("/dev/shm/neontop-fps-{me}"));
}
