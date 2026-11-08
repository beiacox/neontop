//! Estado completo de la aplicación a partir de las muestras: historial de gráficas, juegos,
//! energía, alertas y procesos. Lo usan igual la ventana y el servidor web.

use std::sync::Arc;

use crate::alerts::{Alerts, Fired};
use crate::collectors::processes::ProcSnapshot;
use crate::collectors::system::{self, SystemInfo};
use crate::energy_log::EnergyLog;
use crate::fps::Games;
use crate::history::History;
use crate::procs::Procs;
use crate::sampler::Snapshot;
use crate::sessions::Sessions;

pub struct Model {
    pub info: SystemInfo,
    /// Última muestra.
    pub snap: Arc<Snapshot>,
    /// Muestras recibidas.
    pub samples: u64,
    pub history: History,
    pub games: Games,
    pub energy: EnergyLog,
    pub alerts: Alerts,
    pub procs: Procs,
}

impl Model {
    /// Con `persist`, el historial de partidas, la energía diaria y las alertas se leen y guardan
    /// en las carpetas de NeonTop; sin él, todo queda en memoria (pruebas, modo demostración).
    pub fn new(info: SystemInfo, persist: bool) -> Self {
        let data = persist.then(crate::paths::data_dir).flatten();
        let config = persist.then(crate::paths::config_dir).flatten();
        Model {
            info,
            snap: Arc::new(Snapshot::default()),
            samples: 0,
            history: History::new(),
            games: Games::new(data.clone().map(|d| Sessions::new(Some(d)))),
            energy: EnergyLog::load(data.map(|d| d.join("energy.json"))),
            alerts: Alerts::load(config.map(|d| d.join("alerts.json"))),
            procs: Procs::default(),
        }
    }

    pub fn for_this_machine(persist: bool) -> Self {
        Self::new(system::info(), persist)
    }

    /// Procesa una muestra. Devuelve las alertas que se acaban de disparar.
    pub fn ingest(&mut self, s: Arc<Snapshot>) -> Vec<Fired> {
        self.history.ingest(&s);
        self.games.ingest(&s, &mut self.history);
        self.history.end();
        self.energy.ingest(&s);
        let fired = self.alerts.check(&s);
        self.alerts.expire(s.ts);
        self.snap = s;
        self.samples += 1;
        fired
    }

    pub fn ingest_procs(&mut self, p: Arc<ProcSnapshot>) {
        self.procs.ingest(p);
        // consumo estimado de cada proceso (pestaña Energía)
        let rows = crate::energy_log::estimate_procs(&self.snap, &self.procs.list.procs, &self.snap.gpus);
        self.energy.accumulate_procs(&rows, self.snap.ts);
    }

    /// Guarda lo pendiente (al cerrar).
    pub fn flush(&mut self) {
        self.energy.save();
        self.alerts.save();
    }
}
