# NeonTop

Monitor de recursos tipo btop, estética neón, FPS de juegos. Repo público `beiacox/neontop`.
**Rama `rust-gpui` (actual) = todo Rust con GPUI**; `main` = versión Tauri + Svelte (0.13).
El historial completo de decisiones está en la memoria `neontop-project`.

- Desarrollo: `cargo run -p neontop` (`-- --demo --tab <id>` con datos simulados); `cargo test --workspace`.
  Cargo fuera del sandbox o con `CARGO_HOME`; tests con `CARGO_TARGET_DIR=target/tests` si hay otra build.
- Capas de 32 bits: `cargo build --release --target i686-unknown-linux-gnu -p neontop-layer -p neontop-gl`.
- Paquete Arch: `packaging/arch` (split `neontop` genérico + `neontop-native`; verificar genérico con objdump).
  No borrar paquetes viejos en el mismo comando que `makepkg`.
- QA visual sin tocar el escritorio: `dbus-run-session -- tools/qa-shot.sh out.png 1600 1000 6 target/debug/neontop --demo --tab cpu`
  (KWin anidado). No capturar la pantalla del usuario con spectacle.
- Probar con datos reales aislando `XDG_CONFIG_HOME`/`XDG_DATA_HOME` y `fpsAuto=false` (si no, toca Steam/Heroic).
- El sandbox tiene su propio espacio de PIDs: para ver procesos del usuario, salir del sandbox.
- Medir consumo siempre en release.
