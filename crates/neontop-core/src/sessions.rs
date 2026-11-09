//! Historial de partidas y benchmarks: un índice con los resúmenes (index.json) y un fichero por
//! registro con los datos completos para volver a dibujar su vista (series en el tiempo,
//! histograma de tiempos de fotograma, fotogramas de los benchmarks…). El formato de esos datos
//! lo define la interfaz (games.svelte.ts); aquí solo se guardan y se leen.
//!
//! Va en ~/.local/share/com.neontop.app/sessions para no depender del almacenamiento del
//! navegador (limitado, y distinto en el teléfono).

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Registros que se conservan en el índice (los más recientes).
const KEEP: usize = 1000;

pub struct Sessions {
    dir: Option<PathBuf>,
    lock: Mutex<()>,
}

fn valid_id(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err("identificador no válido".into())
    }
}

impl Sessions {
    pub fn new(data_dir: Option<PathBuf>) -> Self {
        let dir = data_dir.map(|d| d.join("sessions"));
        if let Some(d) = &dir {
            let _ = fs::create_dir_all(d);
        }
        Sessions { dir, lock: Mutex::new(()) }
    }

    fn dir(&self) -> Result<&PathBuf, String> {
        self.dir.as_ref().ok_or_else(|| "sin carpeta de datos".to_string())
    }

    fn read_index(&self) -> Vec<Value> {
        self.dir
            .as_ref()
            .and_then(|d| fs::read_to_string(d.join("index.json")).ok())
            .and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok())
            .unwrap_or_default()
    }

    fn write_index(&self, list: &[Value]) -> Result<(), String> {
        let d = self.dir()?;
        // escritura atómica: un corte a mitad no deja el índice roto
        let tmp = d.join("index.json.tmp");
        fs::write(&tmp, serde_json::to_vec(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        fs::rename(tmp, d.join("index.json")).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<Value> {
        let _g = self.lock.lock().unwrap();
        self.read_index()
    }

    /// Guarda un registro: el resumen va al principio del índice y los datos a su fichero.
    pub fn save(&self, summary: Value, detail: Option<Value>) -> Result<(), String> {
        let id = summary.get("id").and_then(Value::as_str).ok_or("falta el id")?.to_string();
        valid_id(&id)?;
        let _g = self.lock.lock().unwrap();
        let d = self.dir()?;
        if let Some(detail) = detail {
            fs::write(d.join(format!("{id}.json")), serde_json::to_vec(&detail).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        let mut list = self.read_index();
        list.retain(|v| v.get("id").and_then(Value::as_str) != Some(&id));
        list.insert(0, summary);
        // los registros que salen del índice se borran también del disco
        for old in list.drain(KEEP.min(list.len())..) {
            if let Some(oid) = old.get("id").and_then(Value::as_str) {
                let _ = fs::remove_file(d.join(format!("{oid}.json")));
            }
        }
        self.write_index(&list)
    }

    pub fn load(&self, id: &str) -> Result<Value, String> {
        valid_id(id)?;
        let s = fs::read_to_string(self.dir()?.join(format!("{id}.json"))).map_err(|_| crate::t!("esta partida no tiene datos detallados").to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        valid_id(id)?;
        let _g = self.lock.lock().unwrap();
        let _ = fs::remove_file(self.dir()?.join(format!("{id}.json")));
        let mut list = self.read_index();
        list.retain(|v| v.get("id").and_then(Value::as_str) != Some(id));
        self.write_index(&list)
    }

    /// Importa los resúmenes que versiones anteriores guardaban en el navegador (solo si el
    /// historial del disco está vacío, para no duplicar).
    pub fn import(&self, items: Vec<Value>) -> Result<usize, String> {
        let _g = self.lock.lock().unwrap();
        if !self.read_index().is_empty() {
            return Ok(0);
        }
        let list: Vec<Value> = items.into_iter().filter(|v| v.get("id").and_then(Value::as_str).is_some_and(|i| valid_id(i).is_ok())).take(KEEP).collect();
        let n = list.len();
        self.write_index(&list)?;
        Ok(n)
    }
}
