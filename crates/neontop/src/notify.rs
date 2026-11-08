//! Notificaciones del escritorio (org.freedesktop.Notifications) para las alertas.

use neontop_core::alerts::Fired;

pub fn send(fired: &[Fired]) {
    for f in fired.iter().filter(|f| f.desktop) {
        let (title, body) = (format!("NeonTop · {}", f.title), f.body.clone());
        // en otro hilo: el servicio de notificaciones puede tardar en responder
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new().appname("NeonTop").summary(&title).body(&body).icon("neontop").show();
        });
    }
}
