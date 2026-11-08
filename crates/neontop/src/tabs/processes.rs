//! Procesos: tabla ordenable (lista o árbol plegable) con búsqueda, detalle del proceso elegido
//! con señales y prioridad, y gráficas de quién gasta CPU y memoria.

use std::collections::{HashMap, HashSet};

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, Entity, FontWeight, Hsla, SharedString, Subscription, UniformListScrollHandle, Window};
use neontop_core::collectors::processes::ProcInfo;
use neontop_core::format as fmt;
use neontop_core::procs::{state as proc_state, SIGNALS};

use super::y_bytes;
use crate::app::Root;
use crate::charts::{self, Plot, XAxis};
use crate::state::Notice;
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::kit::{self, Panel};
use crate::ui::theme::{fade, Th};

const ROW_H: f32 = 22.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Name,
    Pid,
    User,
    State,
    Cpu,
    Mem,
    Threads,
    Read,
    Write,
    Time,
}

const COLS: [(Col, &str, f32); 10] = [
    (Col::Name, "Nombre", 0.0),
    (Col::Pid, "PID", 64.0),
    (Col::User, "Usuario", 90.0),
    (Col::State, "Est.", 40.0),
    (Col::Cpu, "CPU", 64.0),
    (Col::Mem, "Memoria", 84.0),
    (Col::Threads, "Hilos", 52.0),
    (Col::Read, "Lectura", 84.0),
    (Col::Write, "Escritura", 84.0),
    (Col::Time, "T. CPU", 80.0),
];

/// Una fila de la tabla (el proceso y su posición en el árbol).
#[derive(Clone)]
pub struct Row {
    pub p: ProcInfo,
    pub guide: String,
    pub has_children: bool,
    pub collapsed: bool,
}

pub struct ProcsView {
    pub search: Entity<TextInput>,
    pub tree: bool,
    pub show_kernel: bool,
    /// % de un núcleo (como top) o % del total de la CPU.
    pub per_core: bool,
    pub sort: Col,
    pub desc: bool,
    pub selected: Option<u32>,
    last_seen: Option<ProcInfo>,
    collapsed: HashSet<u32>,
    confirming: Option<&'static str>,
    nice: i32,
    msg: Option<Notice>,
    rows: Vec<Row>,
    scroll: UniformListScrollHandle,
    _sub: Subscription,
}

impl ProcsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Root>) -> Self {
        let search = cx.new(|cx| TextInput::new(cx, "Filtrar por nombre, comando, usuario o PID…"));
        let sub = cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify());
        ProcsView {
            search,
            tree: false,
            show_kernel: false,
            per_core: true,
            sort: Col::Cpu,
            desc: true,
            selected: None,
            last_seen: None,
            collapsed: HashSet::new(),
            confirming: None,
            nice: 0,
            msg: None,
            rows: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            _sub: sub,
        }
    }
}

fn cmp(a: &ProcInfo, b: &ProcInfo, k: Col, desc: bool) -> std::cmp::Ordering {
    let o = match k {
        Col::Pid => a.pid.cmp(&b.pid),
        Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        Col::User => a.user.cmp(&b.user),
        Col::State => a.state.cmp(&b.state),
        Col::Cpu => a.cpu.total_cmp(&b.cpu),
        Col::Mem => a.mem.cmp(&b.mem),
        Col::Threads => a.threads.cmp(&b.threads),
        Col::Read => a.read_rate.unwrap_or(-1.0).total_cmp(&b.read_rate.unwrap_or(-1.0)),
        Col::Write => a.write_rate.unwrap_or(-1.0).total_cmp(&b.write_rate.unwrap_or(-1.0)),
        Col::Time => a.cpu_time.total_cmp(&b.cpu_time),
    };
    (if desc { o.reverse() } else { o }).then(a.pid.cmp(&b.pid))
}

fn matches(p: &ProcInfo, q: &str) -> bool {
    p.name.to_lowercase().contains(q) || p.cmd.to_lowercase().contains(q) || p.user.to_lowercase().contains(q) || p.pid.to_string() == q
}

/// Filas visibles: lista ordenada o árbol (con un filtro, las coincidencias y sus antepasados).
pub fn build_rows(all: &[ProcInfo], query: &str, tree: bool, show_kernel: bool, sort: Col, desc: bool, collapsed: &HashSet<u32>) -> Vec<Row> {
    let q = query.trim().to_lowercase();
    let list: Vec<&ProcInfo> = all.iter().filter(|p| show_kernel || !p.kernel).collect();
    if !tree {
        let mut v: Vec<&ProcInfo> = list.into_iter().filter(|p| q.is_empty() || matches(p, &q)).collect();
        v.sort_by(|a, b| cmp(a, b, sort, desc));
        return v.into_iter().map(|p| Row { p: p.clone(), guide: String::new(), has_children: false, collapsed: false }).collect();
    }
    let by_pid: HashMap<u32, &ProcInfo> = all.iter().map(|p| (p.pid, p)).collect();
    let keep: Option<HashSet<u32>> = (!q.is_empty()).then(|| {
        let mut keep = HashSet::new();
        for p in list.iter().filter(|p| matches(p, &q)) {
            let mut cur = Some(*p);
            while let Some(c) = cur {
                if !keep.insert(c.pid) {
                    break;
                }
                cur = by_pid.get(&c.ppid).copied().filter(|x| x.pid != c.pid);
            }
        }
        keep
    });
    let visible: Vec<&ProcInfo> = list.into_iter().filter(|p| keep.as_ref().is_none_or(|k| k.contains(&p.pid))).collect();
    let pids: HashSet<u32> = visible.iter().map(|p| p.pid).collect();
    let mut children: HashMap<u32, Vec<&ProcInfo>> = HashMap::new();
    let mut roots = Vec::new();
    for p in &visible {
        if pids.contains(&p.ppid) && p.ppid != p.pid {
            children.entry(p.ppid).or_default().push(p);
        } else {
            roots.push(*p);
        }
    }
    let mut out = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn walk(p: &ProcInfo, depth: usize, prefix: &str, last: bool, children: &HashMap<u32, Vec<&ProcInfo>>, collapsed: &HashSet<u32>, filtering: bool, sort: Col, desc: bool, out: &mut Vec<Row>) {
        let mut kids = children.get(&p.pid).cloned().unwrap_or_default();
        kids.sort_by(|a, b| cmp(a, b, sort, desc));
        let is_collapsed = collapsed.contains(&p.pid) && !filtering;
        let guide = if depth == 0 { String::new() } else { format!("{prefix}{}", if last { "└─" } else { "├─" }) };
        out.push(Row { p: p.clone(), guide, has_children: !kids.is_empty(), collapsed: is_collapsed });
        if is_collapsed {
            return;
        }
        let next = if depth == 0 { String::new() } else { format!("{prefix}{}", if last { "  " } else { "│ " }) };
        let n = kids.len();
        for (i, k) in kids.into_iter().enumerate() {
            walk(k, depth + 1, &next, i + 1 == n, children, collapsed, filtering, sort, desc, out);
        }
    }
    roots.sort_by(|a, b| cmp(a, b, sort, desc));
    let n = roots.len();
    for (i, r) in roots.into_iter().enumerate() {
        walk(r, 0, "", i + 1 == n, &children, collapsed, !q.is_empty(), sort, desc, &mut out);
    }
    out
}

fn cell(w: f32, right: bool) -> gpui::Div {
    let d = div().flex_shrink_0().overflow_hidden().whitespace_nowrap();
    let d = if w > 0.0 { d.w(px(w)) } else { d.flex_1().min_w(px(140.)) };
    if right { d.flex().justify_end().pr(px(8.)) } else { d }
}

fn send_signal(this: &mut Root, pid: u32, sig: &'static str, cx: &mut Context<Root>) {
    // terminar o matar pide confirmación (segundo clic)
    if (sig == "TERM" || sig == "KILL") && this.procs.confirming != Some(sig) {
        this.procs.confirming = Some(sig);
        cx.notify();
        return;
    }
    this.procs.confirming = None;
    let (elevate, demo) = {
        let s = this.state.read(cx);
        (s.ui.admin_actions, s.demo)
    };
    let label = SIGNALS.iter().find(|s| s.0 == sig).map(|s| s.1).unwrap_or(sig);
    let view = cx.entity().downgrade();
    cx.spawn(async move |_, cx| {
        let res = cx
            .background_executor()
            .spawn(async move {
                if demo {
                    return Ok(());
                }
                match neontop_core::collectors::processes::send_signal(pid, sig) {
                    Err(e) if elevate && (e.contains("no permitida") || e.contains("Permiso denegado")) => neontop_core::helper::elevate(&["signal", &pid.to_string(), sig]),
                    other => other,
                }
            })
            .await;
        let _ = view.update(cx, |this, cx| {
            this.procs.msg = Some(match res {
                Ok(()) => Notice::Ok(format!("{label}: señal enviada a {pid}")),
                Err(e) => Notice::Err(e),
            });
            cx.notify();
        });
    })
    .detach();
}

fn renice(this: &mut Root, pid: u32, cx: &mut Context<Root>) {
    let nice = this.procs.nice;
    let (elevate, demo) = {
        let s = this.state.read(cx);
        (s.ui.admin_actions, s.demo)
    };
    let view = cx.entity().downgrade();
    cx.spawn(async move |_, cx| {
        let res = cx
            .background_executor()
            .spawn(async move {
                if demo {
                    return Ok(());
                }
                match neontop_core::collectors::processes::renice(pid, nice) {
                    Err(e) if elevate && (e.contains("no permitida") || e.contains("Permiso denegado")) => neontop_core::helper::elevate(&["renice", &pid.to_string(), &nice.to_string()]),
                    other => other,
                }
            })
            .await;
        let _ = view.update(cx, |this, cx| {
            this.procs.msg = Some(match res {
                Ok(()) => Notice::Ok(format!("Prioridad de {pid}: nice {nice}")),
                Err(e) => Notice::Err(e),
            });
            cx.notify();
        });
    })
    .detach();
}

fn table(root: &mut Root, cpu_div: f32, cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let v = &root.procs;
    let header = div()
        .flex()
        .h(px(24.))
        .items_center()
        .border_b_1()
        .border_color(th.border(th.cyan))
        .text_size(px(10.))
        .font_family(th.mono.clone())
        .text_color(th.muted)
        .children(COLS.iter().map(|&(col, label, w)| {
            let on = v.sort == col;
            let right = !matches!(col, Col::Name | Col::User | Col::State);
            let arrow = if on { if v.desc { " ▼" } else { " ▲" } } else { "" };
            cell(w, right)
                .id(SharedString::from(format!("h-{label}")))
                .cursor_pointer()
                .when(on, |d| d.text_color(th.cyan))
                .hover(|s| s.text_color(th.text))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let p = &mut this.procs;
                    if p.sort == col {
                        p.desc = !p.desc;
                    } else {
                        p.sort = col;
                        // texto ascendente; números descendente
                        p.desc = !matches!(col, Col::Name | Col::User | Col::State | Col::Pid);
                    }
                    cx.notify();
                }))
                .child(format!("{}{arrow}", th.title(label)))
        }));
    let n = v.rows.len();
    let list = uniform_list(
        "proc-rows",
        n,
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            let th = Th::get(cx).clone();
            let sel = this.procs.selected;
            range
                .filter_map(|ix| this.procs.rows.get(ix).cloned().map(|r| (ix, r)))
                .map(|(ix, r)| {
                    let p = &r.p;
                    let pid = p.pid;
                    let cpu = p.cpu / cpu_div;
                    let on = sel == Some(pid);
                    let (_, sc) = proc_state(&p.state);
                    let toggle = r.has_children.then(|| {
                        div()
                            .id(SharedString::from(format!("t-{pid}")))
                            .w(px(14.))
                            .cursor_pointer()
                            .text_color(th.cyan)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.procs.collapsed.remove(&pid) {
                                    this.procs.collapsed.insert(pid);
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .child(if r.collapsed { "▸" } else { "▾" })
                    });
                    div()
                        .id(ix)
                        .w_full()
                        .flex()
                        .items_center()
                        .h(px(ROW_H))
                        .text_size(px(11.5))
                        .font_family(th.mono.clone())
                        .cursor_pointer()
                        .when(on, |d| d.bg(fade(th.cyan, 0.16)))
                        .when(!on && ix % 2 == 1, |d| d.bg(fade(th.text, 0.025)))
                        .hover(|s| s.bg(fade(th.cyan, 0.08)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.procs.selected = if this.procs.selected == Some(pid) { None } else { Some(pid) };
                            this.procs.confirming = None;
                            this.procs.msg = None;
                            if let Some(p) = this.state.read(cx).model.procs.get(pid) {
                                this.procs.nice = p.nice;
                            }
                            cx.notify();
                        }))
                        .child(
                            cell(0.0, false)
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(div().text_color(fade(th.muted, 0.7)).child(r.guide.clone()))
                                .children(toggle)
                                .child(div().truncate().text_color(if p.kernel { th.muted } else { th.text }).child(p.name.clone())),
                        )
                        .child(cell(64., true).text_color(th.muted).child(pid.to_string()))
                        .child(cell(90., false).truncate().text_color(if p.uid == 0 { th.amber } else { th.text }).child(p.user.clone()))
                        .child(cell(40., false).text_color(th.slot(sc)).child(p.state.clone()))
                        .child(cell(64., true).text_color(th.load_color(cpu)).child(format!("{cpu:.1}")))
                        .child(cell(84., true).text_color(th.magenta).child(fmt::bytes(p.mem as f64)))
                        .child(cell(52., true).child(p.threads.to_string()))
                        .child(cell(84., true).text_color(th.muted).child(p.read_rate.map(fmt::rate).unwrap_or("—".into())))
                        .child(cell(84., true).text_color(th.muted).child(p.write_rate.map(fmt::rate).unwrap_or("—".into())))
                        .child(cell(80., true).text_color(th.muted).child(fmt::duration(p.cpu_time)))
                })
                .collect::<Vec<_>>()
        }),
    )
    .track_scroll(v.scroll.clone())
    .w_full()
    .h(px(520.));
    div().flex().flex_col().w_full().min_w(px(860.)).child(header).child(list)
}

fn detail(root: &Root, p: &ProcInfo, alive: bool, cpu_div: f32, all: &[ProcInfo], uptime: f64, cx: &mut Context<Root>) -> impl IntoElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let h = st.model.procs.hist.get(&p.pid).cloned();
    let cpu = p.cpu / cpu_div;
    let parent = all.iter().find(|x| x.pid == p.ppid).map(|x| (x.pid, x.name.clone()));
    let children = all.iter().filter(|x| x.ppid == p.pid).count();
    let (state_label, _) = proc_state(&p.state);
    let pid = p.pid;
    let spark = |label: &str, value: String, color: Hsla, data: Option<Vec<f32>>, max: Option<f32>| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(div().flex().justify_between().text_size(px(10.5)).child(kit::muted(th.title(label), cx)).child(div().text_color(color).font_family(th.mono.clone()).child(value)))
            .children(data.filter(|d| d.len() > 1).map(|d| charts::sparkline(d, color, 38., max)))
    };
    let mut col = div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(10.))
                .child(div().font_family(th.display.clone()).font_weight(FontWeight::BOLD).text_size(px(17.)).text_color(th.load_color(cpu)).truncate().child(p.name.clone()))
                .child(kit::muted(format!("PID {pid}"), cx))
                .when(!alive, |d| d.child(kit::badge("TERMINADO", th.red))),
        )
        .child(div().p(px(6.)).bg(fade(th.bg, 0.6)).rounded(px(4.)).text_size(px(10.5)).font_family(th.mono.clone()).text_color(th.muted).child(p.cmd.clone()))
        .child(spark("CPU", format!("{cpu:.1}%"), th.load_color(cpu), h.as_ref().map(|h| h.cpu.iter().map(|v| v / cpu_div).collect()), Some(10f32.max(h.as_ref().map(|h| h.cpu.iter().copied().fold(0.0, f32::max) / cpu_div).unwrap_or(0.0)))))
        .child(spark("Memoria", fmt::bytes(p.mem as f64), th.magenta, h.as_ref().map(|h| h.mem.iter().copied().collect()), None))
        .child(spark(
            "E/S disco",
            if p.read_rate.is_none() { "sin permiso".into() } else { fmt::rate(p.read_rate.unwrap_or(0.0) + p.write_rate.unwrap_or(0.0)) },
            th.amber,
            h.as_ref().filter(|_| p.read_rate.is_some()).map(|h| h.io.iter().copied().collect()),
            None,
        ))
        .child(kit::kv_grid(
            vec![
                ("Estado".into(), format!("{} · {state_label}", p.state)),
                ("Usuario".into(), format!("{} (uid {})", p.user, p.uid)),
                ("Padre".into(), parent.as_ref().map(|(pp, n)| format!("{n} ({pp})")).unwrap_or(p.ppid.to_string()) + &if children > 0 { format!(" · {children} hijos") } else { String::new() }),
                ("Hilos".into(), p.threads.to_string()),
                ("Prioridad".into(), format!("{} · nice {}", p.priority, p.nice)),
                ("Virtual".into(), fmt::bytes(p.virt as f64)),
                ("T. CPU".into(), fmt::duration(p.cpu_time)),
                ("Iniciado".into(), format!("hace {}", fmt::duration((uptime - p.start).max(0.0)))),
            ],
            false,
            cx,
        ));
    if let Some((ppid, pname)) = parent {
        col = col.child(kit::button("go-parent", format!("Ir al padre: {pname}"), th.cyan, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.procs.selected = Some(ppid);
            cx.notify();
        })));
    }
    if alive {
        let confirming = root.procs.confirming;
        col = col
            .child(kit::muted("SEÑALES", cx).text_size(px(9.5)))
            .child(div().flex().flex_wrap().gap(px(6.)).children(SIGNALS.iter().map(|&(sig, label)| {
                let danger = sig == "KILL" || sig == "TERM";
                let text = if confirming == Some(sig) { format!("¿{label}? Confirmar") } else { label.to_string() };
                kit::button(SharedString::from(format!("sig-{sig}")), text, if danger { th.red } else { th.cyan }, cx)
                    .when(confirming == Some(sig), |d| d.bg(fade(th.red, 0.25)))
                    .on_click(cx.listener(move |this, _, _, cx| send_signal(this, pid, sig, cx)))
            })))
            .child(kit::muted("PRIORIDAD (NICE)", cx).text_size(px(9.5)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(kit::stepper(
                        "nice",
                        root.procs.nice.to_string(),
                        cx,
                        cx.listener(|this, _, _, cx| {
                            this.procs.nice = (this.procs.nice - 1).max(-20);
                            cx.notify();
                        }),
                        cx.listener(|this, _, _, cx| {
                            this.procs.nice = (this.procs.nice + 1).min(19);
                            cx.notify();
                        }),
                    ))
                    .child(kit::button("nice-apply", "Aplicar", th.lime, cx).when(root.procs.nice == p.nice, |d| d.opacity(0.4)).on_click(cx.listener(move |this, _, _, cx| renice(this, pid, cx)))),
            )
            .child(kit::muted("Valores más bajos = más prioridad. Bajar el nice requiere privilegios de root.", cx));
    }
    col.children(root.procs.msg.clone().map(|m| match m {
        Notice::Ok(s) => div().text_color(th.lime).child(s),
        Notice::Err(s) => div().text_color(th.red).child(s),
    }))
}

pub fn render(root: &mut Root, _window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let th = Th::get(cx).clone();
    let st = root.state.read(cx);
    let list = st.model.procs.list.clone();
    let ncores = st.model.info.logical_cores.max(1) as f32;
    let uptime = st.model.snap.uptime;
    let running_hist = super::hv(st, "cpu.running");
    let _ = st;
    let all = &list.procs;
    let query = root.procs.search.read(cx).text().to_string();
    let v = &root.procs;
    root.procs.rows = build_rows(all, &query, v.tree, v.show_kernel, v.sort, v.desc, &v.collapsed);
    let cpu_div = if root.procs.per_core { 1.0 } else { ncores };

    // el proceso elegido (aunque haya terminado se sigue mostrando su última información)
    let current = root.procs.selected.and_then(|pid| all.iter().find(|p| p.pid == pid).cloned());
    if current.is_some() {
        root.procs.last_seen = current.clone();
    }
    let shown = current.clone().or_else(|| root.procs.last_seen.clone().filter(|p| Some(p.pid) == root.procs.selected));

    let mut by_state: HashMap<&str, usize> = HashMap::new();
    for p in all.iter() {
        *by_state.entry(p.state.as_str()).or_default() += 1;
    }
    let users: HashSet<&str> = all.iter().map(|p| p.user.as_str()).collect();
    let threads: u32 = all.iter().map(|p| p.threads).sum();
    let kernel = all.iter().filter(|p| p.kernel).count();
    let cpu_sum = all.iter().map(|p| p.cpu).sum::<f32>() / ncores;
    let get = |k: &str| *by_state.get(k).unwrap_or(&0);
    let stats = vec![
        kit::stat("Procesos", all.len().to_string(), format!("{kernel} del kernel"), th.cyan, None, cx).into_any_element(),
        kit::stat("Hilos", threads.to_string(), "", th.violet, None, cx).into_any_element(),
        kit::stat("Ejecutando", get("R").to_string(), "", th.lime, Some(running_hist), cx).into_any_element(),
        kit::stat("Durmiendo", (get("S") + get("I")).to_string(), "", th.blue, None, cx).into_any_element(),
        kit::stat("Espera disco", get("D").to_string(), "", th.amber, None, cx).into_any_element(),
        kit::stat("Zombis", get("Z").to_string(), "", if get("Z") > 0 { th.red } else { th.muted }, None, cx).into_any_element(),
        kit::stat("Usuarios", users.len().to_string(), "", th.teal, None, cx).into_any_element(),
        kit::stat("CPU procesos", fmt::pct(cpu_sum as f64, 1), "suma / núcleos", th.load_color(cpu_sum), None, cx).into_any_element(),
    ];

    let pv = &root.procs;
    let toolbar = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.))
        .child(div().flex_1().min_w(px(220.)).child(pv.search.clone()))
        .child(kit::toggle("p-tree", pv.tree, "Árbol", cx, cx.listener(|this, _, _, cx| {
            this.procs.tree = !this.procs.tree;
            cx.notify();
        })))
        .child(kit::toggle("p-kernel", pv.show_kernel, "Hilos del kernel", cx, cx.listener(|this, _, _, cx| {
            this.procs.show_kernel = !this.procs.show_kernel;
            cx.notify();
        })))
        .child(kit::toggle("p-core", pv.per_core, "% por núcleo", cx, cx.listener(|this, _, _, cx| {
            this.procs.per_core = !this.procs.per_core;
            cx.notify();
        })))
        .when(pv.tree && !pv.collapsed.is_empty(), |d| {
            d.child(kit::button("p-expand", "Desplegar todo", th.cyan, cx).on_click(cx.listener(|this, _, _, cx| {
                this.procs.collapsed.clear();
                cx.notify();
            })))
        });
    let table_panel = Panel::new("Procesos", th.cyan)
        .span(if shown.is_some() { 3 } else { 4 })
        .extra(format!("{} filas", root.procs.rows.len()))
        .child(toolbar)
        .child(if all.is_empty() { kit::empty("LEYENDO /proc…", cx).into_any_element() } else { div().id("ptable").overflow_x_scroll().child(table(root, cpu_div, cx)).into_any_element() });

    let mut g = kit::grid(cx).child(kit::stats_row(stats, cx)).child(table_panel);
    if let Some(p) = shown.clone() {
        g = g.child(
            Panel::new("Detalle", th.magenta)
                .extra(div().id("close-detail").cursor_pointer().child("✕").on_click(cx.listener(|this, _, _, cx| {
                    this.procs.selected = None;
                    cx.notify();
                })))
                .child(detail(root, &p, current.is_some(), cpu_div, all, uptime, cx)),
        );
    }

    // quién gasta: top CPU, CPU × memoria, memoria por aplicación, CPU por usuario y estados
    let mut top: Vec<&ProcInfo> = all.iter().collect();
    top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let top_rows = div().flex().flex_col().gap(px(4.)).children(top.iter().take(12).map(|p| {
        let pid = p.pid;
        let cpu = p.cpu / cpu_div;
        div()
            .id(SharedString::from(format!("top-{pid}")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.procs.selected = Some(pid);
                cx.notify();
            }))
            .child(kit::meter_row(format!("{} ({pid})", p.name), (cpu / if cpu_div > 1.0 { 100.0 } else { 100.0 * ncores.min(4.0) }).min(1.0), format!("{cpu:.1}%"), th.load_color(cpu), cx))
    }));
    let max_mem = all.iter().map(|p| p.mem).max().unwrap_or(1) as f32;
    let dots: Vec<(f64, f32, Hsla)> = all.iter().filter(|p| p.cpu > 0.05 || p.mem as f32 > max_mem * 0.02).map(|p| (p.cpu as f64 / cpu_div as f64, p.mem as f32, if p.uid == 0 { th.amber } else { th.cyan })).collect();
    let xmax = dots.iter().map(|d| d.0).fold(1.0, f64::max) * 1.1;
    let scatter = Plot::new(vec![], vec![]).x_range(0.0, xmax).x_axis(XAxis::Value("%")).y_max(max_mem * 1.1).y_fmt(y_bytes).bytes().dots(dots).height(280.);
    let mut by_app: HashMap<&str, u64> = HashMap::new();
    for p in all.iter() {
        *by_app.entry(p.name.as_str()).or_default() += p.mem;
    }
    let mut apps: Vec<(&str, u64)> = by_app.into_iter().collect();
    apps.sort_by(|a, b| b.1.cmp(&a.1));
    let amax = apps.first().map(|a| a.1).unwrap_or(1) as f32;
    let app_rows = div().flex().flex_col().gap(px(4.)).children(apps.iter().take(14).map(|(n, m)| kit::meter_row(n.to_string(), *m as f32 / amax, fmt::bytes(*m as f64), th.magenta, cx)));
    let mut by_user: HashMap<&str, f32> = HashMap::new();
    for p in all.iter() {
        *by_user.entry(p.user.as_str()).or_default() += p.cpu;
    }
    let mut ulist: Vec<(&str, f32)> = by_user.into_iter().filter(|u| u.1 > 0.0).collect();
    ulist.sort_by(|a, b| b.1.total_cmp(&a.1));
    let utot: f32 = ulist.iter().map(|u| u.1).sum::<f32>().max(0.01);
    let mut states: Vec<(&str, usize)> = by_state.into_iter().collect();
    states.sort_by(|a, b| b.1.cmp(&a.1));
    let smax = states.first().map(|s| s.1).unwrap_or(1) as f32;
    let state_rows = div().flex().flex_col().gap(px(5.)).children(states.iter().map(|(s, n)| {
        let (label, c) = proc_state(s);
        kit::meter_row(format!("{s} · {label}"), *n as f32 / smax, n.to_string(), th.slot(c), cx)
    }));
    g.child(Panel::new("Top CPU", th.lime).span(2).extra("clic para seleccionar").child(top_rows))
        .child(Panel::new("CPU × memoria", th.violet).span(2).extra(kit::legend(vec![("usuario".into(), th.cyan), ("root".into(), th.amber)])).child(scatter.into_element()))
        .child(Panel::new("Memoria por aplicación", th.magenta).span(2).extra("suma de sus procesos").child(app_rows))
        .child(
            Panel::new("CPU por usuario", th.cyan)
                .child(charts::ring(ulist.iter().enumerate().map(|(i, u)| (u.1 / utot, th.series(i))).collect(), fmt::pct((cpu_sum) as f64, 0), "CPU total".into(), 200.))
                .child(kit::legend(ulist.iter().enumerate().take(5).map(|(i, u)| (format!("{} {:.0}%", u.0, u.1 / cpu_div), th.series(i))).collect())),
        )
        .child(Panel::new("Estados", th.amber).child(state_rows))
        .into_any_element()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, name: &str, cpu: f32) -> ProcInfo {
        ProcInfo { pid, ppid, name: name.into(), cmd: name.into(), user: "u".into(), cpu, ..Default::default() }
    }

    #[test]
    fn tree_rows_with_guides_and_collapse() {
        let all = vec![p(1, 0, "init", 0.0), p(2, 1, "a", 5.0), p(3, 1, "b", 1.0), p(4, 2, "a-hijo", 0.0)];
        let rows = build_rows(&all, "", true, false, Col::Cpu, true, &HashSet::new());
        let names: Vec<_> = rows.iter().map(|r| (r.p.name.as_str(), r.guide.as_str())).collect();
        assert_eq!(names, vec![("init", ""), ("a", "├─"), ("a-hijo", "│ └─"), ("b", "└─")]);
        assert!(rows[0].has_children && rows[1].has_children && !rows[3].has_children);
        let collapsed: HashSet<u32> = [2].into_iter().collect();
        assert_eq!(build_rows(&all, "", true, false, Col::Cpu, true, &collapsed).len(), 3);
        // con filtro: la coincidencia y sus antepasados (aunque estén plegados)
        let rows = build_rows(&all, "hijo", true, false, Col::Cpu, true, &collapsed);
        assert_eq!(rows.iter().map(|r| r.p.pid).collect::<Vec<_>>(), vec![1, 2, 4]);
        // en lista: ordenada por CPU
        let rows = build_rows(&all, "", false, false, Col::Cpu, true, &HashSet::new());
        assert_eq!(rows[0].p.pid, 2);
    }
}
