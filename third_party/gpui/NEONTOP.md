# GPUI 0.2.2 (con un parche)

Copia de [gpui 0.2.2](https://crates.io/crates/gpui/0.2.2) (Zed Industries, Apache-2.0, ver
`LICENSE-APACHE`) sin los ejemplos, con un único cambio:

- `src/platform/linux/wayland/client.rs` y `src/platform/linux/x11/client.rs`: al cerrarse la última
  ventana, GPUI paraba el bucle de eventos y el programa terminaba. NeonTop necesita seguir vivo en la
  bandeja del sistema (midiendo, con alertas e interfaz web) y volver a abrir la ventana desde ella;
  la decisión de salir la toma la aplicación (`app::on_window_closed` → `cx.quit()`).

Se usa con `[patch.crates-io]` en el `Cargo.toml` del espacio de trabajo.
