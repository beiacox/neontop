//! Núcleo de NeonTop: todo lo que no es interfaz. Lee /proc y /sys, calcula estadísticas
//! (historial, FPS, energía, alertas) y guarda la configuración. Lo usan la ventana (GPUI), el
//! servidor web y el modo ayudante que corre como root.

pub mod collectors;
pub mod config;
pub mod helper;
pub mod launchers;
pub mod sampler;
pub mod sessions;
