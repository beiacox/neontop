//! Panel de Ajustes (a la derecha de la ventana): tema y efectos, muestreo, historial, alertas,
//! exportación, arranque con el sistema, permisos de administrador, acceso web y bandeja.

use gpui::{canvas, div, fill, point, prelude::*, px, size, AnyElement, App, Bounds, Context, Entity, FontWeight, ScrollHandle, SharedString, Subscription, WeakEntity, Window};
use neontop_core::alerts::{multi, Channel};
use neontop_core::config::WINDOWS;
use neontop_core::format as fmt;
use neontop_core::theme::{CATEGORIES, SYSTEM_THEME, THEMES};

use crate::app::Root;
use crate::state::{state, AppState, Notice};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::kit;
use crate::ui::theme::{fade, hex, Th};

pub struct SettingsView {
    pub root: WeakEntity<Root>,
    scroll: ScrollHandle,
    show_key: bool,
    port: Entity<TextInput>,
    msg: Option<Notice>,
    /// URL elegida para el código QR.
    url: Option<String>,
    _subs: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(root: WeakEntity<Root>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let st = state(cx);
        let port_now = st.read(cx).cfg.get().web_port;
        let port = cx.new(|cx| {
            let mut t = TextInput::new(cx, "7340").width(80.);
            t.set_text(port_now.to_string(), cx);
            t
        });
        let subs = vec![
            cx.observe(&st, |_, _, cx| cx.notify()),
            cx.subscribe(&port, |this, input, ev: &InputEvent, cx| {
                if let InputEvent::Submit = ev {
                    let p = input.read(cx).text().trim().parse::<u16>().ok();
                    this.set_web(None, p, None, cx);
                }
            }),
        ];
        SettingsView { root, scroll: ScrollHandle::new(), show_key: false, port, msg: None, url: None, _subs: subs }
    }

    fn st(&self, cx: &App) -> Entity<AppState> {
        state(cx)
    }

    /// Cambia el servidor web (activado, puerto, acciones) y lo reinicia.
    fn set_web(&mut self, enabled: Option<bool>, port: Option<u16>, actions: Option<bool>, cx: &mut Context<Self>) {
        let st = self.st(cx);
        st.update(cx, |s, cx| {
            s.cfg.update(|c| {
                if let Some(e) = enabled {
                    c.web_enabled = e;
                }
                if let Some(p) = port {
                    c.web_port = p.max(1024);
                }
                if let Some(a) = actions {
                    c.web_actions = a;
                }
            });
            if !s.demo {
                s.web.apply();
            }
            cx.notify();
        });
    }

    fn section(title: &str, cx: &App) -> gpui::Div {
        let th = Th::get(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .py(px(12.))
            .border_b_1()
            .border_color(th.border(th.cyan))
            .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(12.5))).text_color(th.cyan).child(th.title(title)))
    }

    fn themes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let cur = self.st(cx).read(cx).ui.theme.clone();
        let swatch = |id: &'static str, name: &str, colors: [u32; 5], cx: &mut Context<Self>| {
            let on = cur == id;
            div()
                .id(SharedString::from(format!("th-{id}")))
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(6.))
                .w(px(116.))
                .cursor_pointer()
                .rounded(px(6.))
                .border_1()
                .border_color(if on { th.cyan } else { th.border(th.cyan) })
                .when(on, |d| d.bg(fade(th.cyan, 0.12)))
                .hover(|s| s.bg(fade(th.cyan, 0.06)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.st(cx).update(cx, |s, cx| s.set_theme(id, cx));
                }))
                .child(div().h(px(22.)).flex().rounded(px(3.)).overflow_hidden().bg(hex(colors[0])).children(colors[1..].iter().map(|c| div().flex_1().m(px(4.)).rounded(px(2.)).bg(hex(*c)))))
                .child(div().text_size(px(11.)).truncate().child(name.to_string()))
        };
        let mut col = div().flex().flex_col().gap(px(8.)).child(div().flex().flex_wrap().gap(px(6.)).child(swatch(SYSTEM_THEME, "Sistema (claro/oscuro)", [0x888888, 0x0d1117, 0xf3f5f8, 0x58a6ff, 0x0969da], cx)));
        for (cat, label) in CATEGORIES {
            let items: Vec<_> = THEMES.iter().filter(|t| t.category == cat).collect();
            col = col.child(kit::muted(label, cx)).child(div().flex().flex_wrap().gap(px(6.)).children(items.into_iter().map(|t| {
                let c = &t.colors;
                swatch(t.id, t.name, [c.bg, c.cyan, c.magenta, c.lime, c.amber], cx)
            })));
        }
        col
    }

    fn alerts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let st = self.st(cx);
        let s = st.read(cx);
        let rules = s.model.alerts.rules.clone();
        let opts = s.model.alerts.opts.clone();
        let log = s.model.alerts.log.clone();
        let snap = s.model.snap.clone();
        let now = fmt::now_ms();
        let upd = |rule: String, f: fn(&mut neontop_core::alerts::Rule)| {
            move |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                let rule = rule.clone();
                this.st(cx).update(cx, |s, cx| {
                    s.model.alerts.update(&rule, f);
                    cx.notify();
                });
            }
        };
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .child(kit::toggle("al-desk", opts.desktop, "Notificaciones del escritorio", cx, cx.listener(|this, _, _, cx| {
                        this.st(cx).update(cx, |s, cx| {
                            s.model.alerts.opts.desktop = !s.model.alerts.opts.desktop;
                            s.model.alerts.save();
                            cx.notify();
                        });
                    })))
                    .child(kit::toggle("al-toast", opts.toasts, "Avisos dentro de NeonTop", cx, cx.listener(|this, _, _, cx| {
                        this.st(cx).update(cx, |s, cx| {
                            s.model.alerts.opts.toasts = !s.model.alerts.opts.toasts;
                            s.model.alerts.save();
                            cx.notify();
                        });
                    }))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(if opts.mute_all > now {
                        kit::button("al-unmute", format!("Silenciadas hasta {} · reactivar", fmt::clock(opts.mute_all)), th.amber, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.st(cx).update(cx, |s, cx| {
                                s.model.alerts.mute_all(0, fmt::now_ms());
                                cx.notify();
                            });
                        }))
                    } else {
                        kit::button("al-mute", "Silenciar todo 1 h", th.amber, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.st(cx).update(cx, |s, cx| {
                                let now = fmt::now_ms();
                                s.model.alerts.mute_all(now + 3_600_000, now);
                                cx.notify();
                            });
                        }))
                    })
                    .child(kit::button("al-reset", "Restablecer", th.muted, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.st(cx).update(cx, |s, cx| {
                            s.model.alerts.reset();
                            cx.notify();
                        });
                    }))),
            );
        for r in rules {
            let id = r.id.clone();
            let step = if r.unit == "°C" { 1.0 } else { 5.0 };
            let items = neontop_core::alerts::probe(&r.id, &snap);
            let r2 = r.clone();
            let mut row = div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .p(px(8.))
                .border_1()
                .border_color(th.border(if r.enabled { th.cyan } else { th.muted }))
                .rounded(px(6.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(kit::toggle(SharedString::from(format!("al-on-{id}")), r.enabled, r.label.clone(), cx, cx.listener(upd(id.clone(), |r| r.enabled = !r.enabled))))
                        .child(div().flex_1())
                        .when(r.muted_until > now, |d| d.child(kit::badge(format!("silenciada hasta {}", fmt::clock(r.muted_until)), th.amber))),
                );
            if r.enabled {
                let idd = id.clone();
                let idi = id.clone();
                let ids = id.clone();
                let ids2 = id.clone();
                let idr = id.clone();
                let idr2 = id.clone();
                let mut controls = div().flex().flex_wrap().items_center().gap(px(12.)).text_size(px(11.));
                if !r.flag {
                    controls = controls.child(kit::muted(if r.below { "por debajo de" } else { "umbral" }, cx)).child(kit::stepper(
                        &format!("thr-{id}"),
                        format!("{}{}", r.threshold, r.unit),
                        cx,
                        cx.listener(move |this, _, _, cx| {
                            let id = idd.clone();
                            this.st(cx).update(cx, |s, cx| {
                                s.model.alerts.update(&id, |r| r.threshold = (r.threshold - step).max(0.0));
                                cx.notify();
                            });
                        }),
                        cx.listener(move |this, _, _, cx| {
                            let id = idi.clone();
                            this.st(cx).update(cx, |s, cx| {
                                s.model.alerts.update(&id, |r| r.threshold = (r.threshold + step).min(if r.unit == "%" { 100.0 } else { 150.0 }));
                                cx.notify();
                            });
                        }),
                    ));
                    controls = controls.child(kit::muted("durante", cx)).child(kit::stepper(
                        &format!("sus-{id}"),
                        format!("{} s", r.sustain),
                        cx,
                        cx.listener(move |this, _, _, cx| {
                            let id = ids.clone();
                            this.st(cx).update(cx, |s, cx| {
                                s.model.alerts.update(&id, |r| r.sustain = r.sustain.saturating_sub(5));
                                cx.notify();
                            });
                        }),
                        cx.listener(move |this, _, _, cx| {
                            let id = ids2.clone();
                            this.st(cx).update(cx, |s, cx| {
                                s.model.alerts.update(&id, |r| r.sustain = (r.sustain + 5).min(600));
                                cx.notify();
                            });
                        }),
                    ));
                }
                controls = controls.child(kit::muted("repetir", cx)).child(kit::stepper(
                    &format!("rep-{id}"),
                    if r.repeat == 0 { "una vez".into() } else { format!("{} min", r.repeat) },
                    cx,
                    cx.listener(move |this, _, _, cx| {
                        let id = idr.clone();
                        this.st(cx).update(cx, |s, cx| {
                            s.model.alerts.update(&id, |r| r.repeat = r.repeat.saturating_sub(5));
                            cx.notify();
                        });
                    }),
                    cx.listener(move |this, _, _, cx| {
                        let id = idr2.clone();
                        this.st(cx).update(cx, |s, cx| {
                            s.model.alerts.update(&id, |r| r.repeat = (r.repeat + 5).min(240));
                            cx.notify();
                        });
                    }),
                ));
                let st2 = st.clone();
                let idc = id.clone();
                controls = controls.child(kit::segmented(
                    &format!("ch-{id}"),
                    vec![(Channel::Both, "ambos".into()), (Channel::App, "app".into()), (Channel::System, "escritorio".into())],
                    &r.channel,
                    cx,
                    move |c, _, cx| {
                        let c = *c;
                        let id = idc.clone();
                        st2.update(cx, |s, cx| {
                            s.model.alerts.update(&id, |r| r.channel = c);
                            cx.notify();
                        });
                    },
                ));
                row = row.child(controls);
                if let Some(what) = multi(&r.id) {
                    let ignored = r2.ignore.clone();
                    row = row.child(kit::muted(format!("Vigila cada uno de los {what}; pulsa para ignorar o volver a vigilar:"), cx)).child(div().flex().flex_wrap().gap(px(6.)).children(
                        items.iter().map(|it| it.key.clone()).chain(ignored.iter().cloned()).collect::<std::collections::BTreeSet<_>>().into_iter().map(|key| {
                            let off = ignored.contains(&key);
                            let (id, k) = (id.clone(), key.clone());
                            kit::button(SharedString::from(format!("ig-{}-{key}", r2.id)), if off { format!("✕ {key}") } else { key.clone() }, if off { th.muted } else { th.cyan }, cx).on_click(cx.listener(move |this, _, _, cx| {
                                let (id, k) = (id.clone(), k.clone());
                                this.st(cx).update(cx, |s, cx| {
                                    if off {
                                        s.model.alerts.unignore(&id, &k);
                                    } else {
                                        s.model.alerts.ignore(&id, &k);
                                    }
                                    cx.notify();
                                });
                            }))
                        }),
                    ));
                }
            }
            col = col.child(row);
        }
        if !log.is_empty() {
            col = col.child(kit::muted("ÚLTIMOS AVISOS", cx).text_size(px(9.5))).children(log.iter().take(8).map(|f| {
                div().flex().gap(px(8.)).text_size(px(11.)).child(div().text_color(th.muted).font_family(th.mono.clone()).child(fmt::clock(f.ts))).child(div().text_color(th.red).child(f.title.clone())).child(kit::muted(f.body.clone(), cx).truncate())
            }));
        }
        col
    }

    fn export(&mut self, all: bool, cx: &mut Context<Self>) {
        let st = self.st(cx);
        let csv = {
            let s = st.read(cx);
            let start = if all { 0 } else { s.window().start };
            s.model.history.to_csv(start)
        };
        let name = format!("neontop-{}.csv", fmt::datetime(fmt::now_ms()).replace(['/', ' ', ':'], ""));
        let dir = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| "/".into());
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let msg = match rx.await {
                Ok(Ok(Some(path))) => match std::fs::write(&path, csv) {
                    Ok(()) => Notice::Ok(format!("Guardado en {}", path.display())),
                    Err(e) => Notice::Err(format!("No se pudo guardar: {e}")),
                },
                Ok(Err(e)) => Notice::Err(e.to_string()),
                _ => return,
            };
            let _ = this.update(cx, |this, cx| {
                this.msg = Some(msg);
                cx.notify();
            });
        })
        .detach();
    }

    fn system(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let st = self.st(cx);
        let s = st.read(cx);
        let cfg = s.cfg.get();
        let ui = s.ui.clone();
        let demo = s.demo;
        let web = s.web.status();
        let rapl = s.model.snap.energy.rapl.clone();
        let rapl_rule = s.model.snap.energy.rapl_rule;
        let membw = s.model.snap.membw.clone();
        let auto = crate::autostart::enabled();
        let urls: Vec<String> = web.urls.iter().map(|u| format!("{u}/?t={}", cfg.web_token)).collect();
        let chosen = self.url.clone().filter(|u| urls.contains(u)).or_else(|| urls.first().cloned());
        let elevate = |args: &'static [&'static str], rescan: bool| {
            move |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                this.st(cx).update(cx, |s, cx| {
                    let ctl = s.ctl.clone();
                    s.run_action(
                        cx,
                        move || {
                            let r = neontop_core::helper::elevate(args);
                            if rescan {
                                ctl.collect.rescan_energy.store(true, std::sync::atomic::Ordering::Relaxed);
                            } else {
                                ctl.collect.restart_membw.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            r
                        },
                        |s, r| s.notice = Some(r.map(|_| Notice::Ok("Hecho.".into())).unwrap_or_else(Notice::Err)),
                    );
                });
            }
        };
        let row = |label: &str| div().flex().flex_wrap().items_center().gap(px(8.)).child(div().w(px(170.)).text_color(th.muted).child(label.to_string()));
        let rapl_row = match rapl.as_str() {
            "ok" => row("Consumo de CPU (RAPL)")
                .child(div().text_color(th.lime).child(if rapl_rule { "permitido siempre" } else { "permitido hasta reiniciar" }))
                .when(!rapl_rule, |d| d.child(kit::button("s-rapl-perm", "Hacer permanente", th.cyan, cx).on_click(cx.listener(elevate(&["rapl-grant", "1"], true)))))
                .child(kit::button("s-rapl-off", "Retirar", th.muted, cx).on_click(cx.listener(elevate(&["rapl-revoke"], true)))),
            "denied" => row("Consumo de CPU (RAPL)")
                .child(div().text_color(th.amber).child("bloqueado"))
                .child(kit::button("s-rapl-tmp", "Permitir hasta reiniciar", th.cyan, cx).on_click(cx.listener(elevate(&["rapl-grant", "0"], true))))
                .child(kit::button("s-rapl-on", "Permitir siempre", th.cyan, cx).on_click(cx.listener(elevate(&["rapl-grant", "1"], true)))),
            _ => row("Consumo de CPU (RAPL)").child(kit::muted("no disponible en este equipo", cx)),
        };
        let membw_row = match membw.status.as_str() {
            "ok" => row("Tráfico de RAM en vivo")
                .child(div().text_color(th.lime).child("activo"))
                .when(membw.enabled, |d| d.child(kit::button("s-mbw-off", "Desactivar", th.muted, cx).on_click(cx.listener(elevate(&["membw-revoke"], false))))),
            "nomodule" | "noperm" => row("Tráfico de RAM en vivo").child(div().text_color(th.amber).child("sin permiso")).child(kit::button("s-mbw-on", "Activar", th.cyan, cx).on_click(cx.listener(elevate(&["membw-grant"], false)))),
            _ => row("Tráfico de RAM en vivo").child(kit::muted(if membw.status == "unsupported" { "no compatible con este procesador".into() } else { membw.message.clone().unwrap_or("no disponible".into()) }, cx)),
        };
        let mut col = div().flex().flex_col();
        col = col.child(
            Self::section("Inicio con el sistema", cx)
                .child(kit::toggle("s-auto", auto, "Abrir NeonTop al iniciar sesión", cx, cx.listener(move |this, _, _, cx| {
                    this.msg = crate::autostart::set(!auto).err().map(Notice::Err);
                    cx.notify();
                })))
                .child(kit::toggle("s-hidden", cfg.start_hidden, "Arrancar oculto en la bandeja (sigue midiendo, con alertas e interfaz web)", cx, cx.listener(|this, _, _, cx| {
                    this.st(cx).update(cx, |s, cx| {
                        s.cfg.update(|c| c.start_hidden = !c.start_hidden);
                        cx.notify();
                    });
                })))
                .child(kit::muted("Si abres NeonTop cuando ya está en marcha, se muestra la ventana existente en lugar de abrir otra.", cx)),
        );
        col = col.child(
            Self::section("Permisos de administrador", cx)
                .child(kit::toggle("s-admin", ui.admin_actions, "Acciones de administrador (terminar o cambiar la prioridad de procesos de otros usuarios o del sistema)", cx, cx.listener(|this, _, _, cx| {
                    this.st(cx).update(cx, |s, cx| s.update_ui(|u| u.admin_actions = !u.admin_actions, cx));
                })))
                .child(kit::muted("Cuando una acción necesite root, el sistema pedirá tu contraseña en su propia ventana (polkit) y la recordará unos minutos. NeonTop nunca la ve ni se ejecuta como root: solo lanza la orden concreta.", cx))
                .when(!demo, |d| d.child(rapl_row).child(membw_row))
                .children(crate::tabs::memory::notice_line(s.notice.clone(), cx)),
        );
        let qr = chosen.as_ref().and_then(|u| qrcode::QrCode::new(u.as_bytes()).ok()).map(|code| {
            let w = code.width();
            let colors: Vec<bool> = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
            canvas(
                |_, _, _| (),
                move |b, _, window, _| {
                    let side = f32::from(b.size.width.min(b.size.height));
                    let m = side / (w as f32 + 4.0);
                    window.paint_quad(fill(b, gpui::white()));
                    for (i, dark) in colors.iter().enumerate() {
                        if *dark {
                            let (x, y) = ((i % w) as f32 + 2.0, (i / w) as f32 + 2.0);
                            window.paint_quad(fill(Bounds::new(point(b.origin.x + px(x * m), b.origin.y + px(y * m)), size(px(m + 0.3), px(m + 0.3))), gpui::black()));
                        }
                    }
                },
            )
            .size(px(170.))
        });
        let show_key = self.show_key;
        let token = cfg.web_token.clone();
        let mut web_sec = Self::section("Acceso web (teléfono y otros equipos)", cx)
            .child(kit::toggle("s-web", cfg.web_enabled, "Servir la interfaz en la red local", cx, cx.listener(move |this, _, _, cx| this.set_web(Some(!cfg.web_enabled), None, None, cx))))
            .child(
                row("Puerto")
                    .child(self.port.clone())
                    .child(kit::button("s-port", "Aplicar", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| {
                        let p = this.port.read(cx).text().trim().parse::<u16>().ok();
                        this.set_web(None, p, None, cx);
                    })))
                    .when(web.running, |d| d.child(div().text_color(th.lime).child(format!("activo · {} {}", web.clients, if web.clients == 1 { "cliente" } else { "clientes" })))),
            )
            .child(kit::toggle("s-webact", cfg.web_actions, "Permitir acciones desde la web (pausar, cambiar el muestreo, terminar procesos)", cx, cx.listener(move |this, _, _, cx| this.set_web(None, None, Some(!cfg.web_actions), cx))))
            .children(web.error.clone().map(|e| div().text_color(th.red).child(e)));
        if demo {
            web_sec = web_sec.child(kit::muted("En el modo demostración el servidor web no se arranca.", cx));
        }
        if cfg.web_enabled && web.running {
            web_sec = web_sec
                .child(
                    div()
                        .flex()
                        .gap(px(14.))
                        .children(qr)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(kit::muted("Escanea el código con el teléfono (conectado a la misma red) o abre uno de estos enlaces:", cx))
                                .children(urls.iter().map(|u| {
                                    let on = Some(u) == chosen.as_ref();
                                    let uu = u.clone();
                                    div()
                                        .id(SharedString::from(format!("url-{u}")))
                                        .cursor_pointer()
                                        .font_family(th.mono.clone())
                                        .text_size(px(10.5))
                                        .text_color(if on { th.cyan } else { th.muted })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.url = Some(uu.clone());
                                            cx.notify();
                                        }))
                                        .child(if show_key { u.clone() } else { u.replace(&token, "•••") })
                                }))
                                .child(
                                    row("Clave")
                                        .child(div().font_family(th.mono.clone()).child(if show_key { token.clone() } else { "•".repeat(12) }))
                                        .child(kit::button("s-show", if show_key { "Ocultar" } else { "Mostrar" }, th.cyan, cx).on_click(cx.listener(|this, _, _, cx| {
                                            this.show_key = !this.show_key;
                                            cx.notify();
                                        })))
                                        .child(kit::button("s-newkey", "Nueva clave", th.amber, cx).on_click(cx.listener(|this, _, _, cx| {
                                            this.st(cx).update(cx, |s, cx| {
                                                s.cfg.update(|c| c.web_token = neontop_core::config::new_token());
                                                cx.notify();
                                            });
                                        }))),
                                ),
                        ),
                )
                .child(kit::muted(
                    format!("Quien tenga el enlace con la clave puede ver los datos del equipo. La conexión no va cifrada (HTTP): úsalo en tu red de casa, no en redes públicas. Si el teléfono no conecta, puede que el cortafuegos bloquee el puerto {}.", cfg.web_port),
                    cx,
                ));
        }
        col = col.child(web_sec).child(
            Self::section("Bandeja del sistema", cx)
                .child(kit::toggle("s-tray", ui.close_to_tray, "Cerrar la ventana la deja en la bandeja", cx, cx.listener(|this, _, _, cx| {
                    this.st(cx).update(cx, |s, cx| s.update_ui(|u| u.close_to_tray = !u.close_to_tray, cx));
                })))
                .child(kit::muted(if crate::tray::available() { "El icono de la bandeja muestra CPU, RAM, GPU y temperatura, y permite pausar o salir." } else { "Este escritorio no tiene bandeja del sistema (StatusNotifier): cerrar la ventana cierra NeonTop." }, cx)),
        );
        col
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = Th::get(cx).clone();
        let st = self.st(cx);
        let s = st.read(cx);
        let ui = s.ui.clone();
        let paused = s.paused();
        let (points, since, bytes, nseries) = s.model.history.stored();
        let compactions = s.model.history.compactions;
        let full_res = neontop_core::history::MAX_POINTS as f64 * ui.interval_ms as f64 / 1000.0;
        let effects_used = (th.theme.style.glow, th.theme.style.scanlines);
        let st2 = st.clone();
        let st3 = st.clone();
        let mut intervals: Vec<(u64, String)> = [100u64, 250, 500, 1000, 2000, 5000, 10_000, 30_000, 60_000].iter().map(|&ms| (ms, fmt::interval(ms))).collect();
        if !intervals.iter().any(|i| i.0 == ui.interval_ms) {
            intervals.push((ui.interval_ms, fmt::interval(ui.interval_ms)));
        }
        let body: Vec<AnyElement> = vec![
            Self::section("Tema", cx).child(self.themes(cx)).into_any_element(),
            Self::section("Efectos", cx)
                .child(kit::toggle("fx-glow", ui.glow, "Brillo neón", cx, cx.listener(move |this, _, _, cx| {
                    let ui = this.st(cx).read(cx).ui.clone();
                    this.st(cx).update(cx, |s, cx| s.set_effects(!ui.glow, ui.scanlines, cx));
                })))
                .child(kit::toggle("fx-scan", ui.scanlines, "Líneas de escaneo CRT", cx, cx.listener(move |this, _, _, cx| {
                    let ui = this.st(cx).read(cx).ui.clone();
                    this.st(cx).update(cx, |s, cx| s.set_effects(ui.glow, !ui.scanlines, cx));
                })))
                .when(!effects_used.0 && !effects_used.1, |d| d.child(kit::muted("El tema actual no usa estos efectos.", cx)))
                .into_any_element(),
            Self::section("Velocidad de actualización", cx)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(24.))).text_color(th.cyan).child(fmt::interval(ui.interval_ms)))
                        .child(kit::muted(format!("{} muestras/min", (60_000 / ui.interval_ms.max(1)).max(1)), cx))
                        .child(kit::button("s-pause", if paused { "▶ Reanudar" } else { "⏸ Pausar" }, if paused { th.amber } else { th.cyan }, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.st(cx).update(cx, |s, cx| s.toggle_pause(cx));
                        }))),
                )
                .child(kit::segmented("s-iv", intervals, &ui.interval_ms, cx, move |ms, _, cx| {
                    let ms = *ms;
                    st2.update(cx, |s, cx| s.set_interval(ms, cx));
                }))
                .child(kit::stepper(
                    "s-ms",
                    format!("{} ms", ui.interval_ms),
                    cx,
                    cx.listener(|this, _, _, cx| {
                        this.st(cx).update(cx, |s, cx| {
                            let ms = s.ui.interval_ms;
                            s.set_interval(ms.saturating_sub(if ms > 1000 { 500 } else { 50 }), cx)
                        });
                    }),
                    cx.listener(|this, _, _, cx| {
                        this.st(cx).update(cx, |s, cx| {
                            let ms = s.ui.interval_ms;
                            s.set_interval(ms + if ms >= 1000 { 500 } else { 50 }, cx)
                        });
                    }),
                ))
                .child(kit::muted("Intervalos cortos (< 500 ms) muestran picos breves pero consumen más CPU.", cx))
                .into_any_element(),
            Self::section("Ventana de historial", cx)
                .child(kit::segmented("s-win", WINDOWS.iter().map(|&w| (w, fmt::window(w))).collect(), &ui.window_sec, cx, move |w, _, cx| {
                    let w = *w;
                    st3.update(cx, |s, cx| s.set_window(w, cx));
                }))
                .child(kit::kv_grid(
                    vec![
                        ("Guardado".into(), format!("{points} muestras · desde {}", if since > 0.0 { fmt::datetime((since * 1000.0) as u64) } else { "—".into() })),
                        ("Memoria".into(), format!("{} en {nseries} series (32 bits)", fmt::bytes(bytes as f64))),
                        (
                            "Resolución".into(),
                            if compactions > 0 { format!("completa ~{}; lo anterior compactado ({compactions}×)", fmt::duration(full_res)) } else { format!("completa durante ~{}; después se compacta lo más antiguo", fmt::duration(full_res)) },
                        ),
                    ],
                    false,
                    cx,
                ))
                .child(kit::button("s-clear", "Borrar historial", th.red, cx).w(px(150.)).on_click(cx.listener(|this, _, _, cx| {
                    this.st(cx).update(cx, |s, cx| {
                        s.model.history.clear();
                        cx.notify();
                    });
                })))
                .into_any_element(),
            Self::section("Alertas", cx).child(self.alerts(cx)).into_any_element(),
            Self::section("Exportar", cx)
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(kit::button("s-csv-win", "CSV · ventana visible", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| this.export(false, cx))))
                        .child(kit::button("s-csv-all", "CSV · todo el historial", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| this.export(true, cx)))),
                )
                .child(kit::muted("Valores sin reducir en unidades base (%, bytes, bytes/s, °C, MHz, W): CPU, núcleos, memoria, red, discos, GPU, sensores y energía.", cx))
                .children(self.msg.clone().and_then(|m| crate::tabs::memory::notice_line(Some(m), cx)))
                .into_any_element(),
            self.system(cx).into_any_element(),
            Self::section("Acerca de", cx)
                .child(kit::kv_grid(
                    vec![
                        ("Versión".into(), env!("CARGO_PKG_VERSION").into()),
                        ("Interfaz".into(), "GPUI (Rust, GPU)".into()),
                        ("Código".into(), "github.com/beiacox/neontop".into()),
                    ],
                    false,
                    cx,
                ))
                .into_any_element(),
        ];
        let root = self.root.clone();
        div()
            .id("settings")
            .occlude()
            .size_full()
            .flex()
            .flex_col()
            .bg(th.bg)
            .border_l_1()
            .border_color(th.cyan)
            .shadow(vec![gpui::BoxShadow { color: fade(gpui::black(), 0.5), offset: point(px(-6.), px(0.)), blur_radius: px(24.), spread_radius: px(0.) }])
            .text_color(th.text)
            .font_family(th.body.clone())
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(18.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(th.border(th.cyan))
                    .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(th.dsize(18.))).child(th.title("Ajustes")))
                    .child(div().id("s-close").cursor_pointer().text_color(th.muted).hover(|s| s.text_color(th.red)).child("✕").on_click(move |_, _, cx| {
                        let _ = root.update(cx, |r, cx| r.close_settings(cx));
                    })),
            )
            .child(div().id("settings-body").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll).px(px(18.)).children(body))
    }
}
