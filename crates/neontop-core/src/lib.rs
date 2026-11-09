//! Núcleo de NeonTop: todo lo que no es interfaz. Lee /proc y /sys, calcula estadísticas
//! (historial, FPS, energía, alertas) y guarda la configuración. Lo usan la ventana (GPUI), el
//! servidor web y el modo ayudante que corre como root.

pub mod alerts;
pub mod collectors;
pub mod config;
pub mod demo;
pub mod energy_log;
pub mod format;
pub mod fps;
pub mod history;
pub mod i18n;
pub mod helper;
pub mod launchers;
pub mod model;
pub mod paths;
pub mod procs;
pub mod sampler;
pub mod sessions;
pub mod theme;
