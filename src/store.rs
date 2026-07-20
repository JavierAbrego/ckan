//! Estado que sobrevive entre ejecuciones: lanes, prompts y notas por pane.
//!
//! Las notas y la lane se anclan al pane_id de tmux (%17), no a la posicion
//! (0:2.1), para que sigan al pane si reordenas la ventana. Lo que no sobrevive
//! es cerrar el pane: ahi la entrada queda huerfana y se limpia al guardar.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const N_LANES: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub text: String,
    pub lane: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Note {
    /// Que estoy haciendo.
    pub doing: String,
    /// Que espero despues.
    pub next: String,
    /// Epoch en segundos de la ultima edicion, para marcar notas rancias.
    pub touched: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    /// Nombres de las 6 lanes. Vacio = sin bautizar.
    pub lane_names: Vec<String>,
    /// Orden de pintado: posicion -> indice de lane.
    ///
    /// Separamos identidad de posicion a proposito. El indice de lane es lo que
    /// usan las teclas 1-6 y lo que guardamos en pane_lane; si al reordenar
    /// permutasemos los indices, la tecla 2 dejaria de llevar a la lane que el
    /// usuario tiene asociada al 2. Aqui solo cambia donde se dibuja.
    #[serde(default = "default_order")]
    pub lane_order: Vec<usize>,
    pub prompts: Vec<Prompt>,
    /// pane_id -> lane
    pub pane_lane: HashMap<String, usize>,
    /// pane_id -> nota
    pub notes: HashMap<String, Note>,
}

fn default_order() -> Vec<usize> {
    (0..N_LANES).collect()
}

impl Default for Store {
    fn default() -> Self {
        Store {
            lane_names: vec![String::new(); N_LANES],
            lane_order: default_order(),
            prompts: Vec::new(),
            pane_lane: HashMap::new(),
            notes: HashMap::new(),
        }
    }
}

fn path() -> PathBuf {
    let mut p = dirs_state();
    p.push("ckan");
    let _ = std::fs::create_dir_all(&p);
    p.push("board.json");
    p
}

fn dirs_state() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_STATE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let mut p = PathBuf::from(home);
    p.push(".local/state");
    p
}

impl Store {
    pub fn load() -> Store {
        match std::fs::read_to_string(path()) {
            Ok(s) => {
                let mut st: Store = serde_json::from_str(&s).unwrap_or_default();
                // Defensivo: si el fichero viene de una version con otro N_LANES.
                st.lane_names.resize(N_LANES, String::new());
                st.fix_order();
                st
            }
            Err(_) => Store::default(),
        }
    }

    /// Guarda descartando entradas de panes que ya no existen.
    pub fn save(&mut self, live_ids: &[String]) {
        self.pane_lane.retain(|k, _| live_ids.contains(k));
        self.notes.retain(|k, _| live_ids.contains(k));
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let p = path();
            // Escritura atomica: si algo peta a media escritura, no corrompemos
            // el fichero bueno.
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn lane_label(&self, i: usize) -> String {
        let n = self.lane_names.get(i).map(|s| s.as_str()).unwrap_or("");
        if n.trim().is_empty() {
            format!("{} · (sin nombre)", i + 1)
        } else {
            format!("{} · {}", i + 1, n.to_uppercase())
        }
    }

    pub fn lane_of(&self, pane_id: &str) -> usize {
        *self.pane_lane.get(pane_id).unwrap_or(&0)
    }

    /// Deja lane_order como una permutacion valida de 0..N_LANES.
    ///
    /// Un fichero editado a mano o de otra version podria traer duplicados,
    /// indices fuera de rango o faltantes; sin esto perderiamos lanes al
    /// pintar. Conservamos el orden de lo que sea valido y anadimos al final
    /// lo que falte.
    pub fn fix_order(&mut self) {
        let mut seen = vec![false; N_LANES];
        let mut clean = Vec::with_capacity(N_LANES);
        for &i in &self.lane_order {
            if i < N_LANES && !seen[i] {
                seen[i] = true;
                clean.push(i);
            }
        }
        for i in 0..N_LANES {
            if !seen[i] {
                clean.push(i);
            }
        }
        self.lane_order = clean;
    }

    /// Posicion de pintado de una lane.
    pub fn position_of(&self, lane: usize) -> usize {
        self.lane_order
            .iter()
            .position(|&l| l == lane)
            .unwrap_or(lane)
    }

    /// Intercambia una lane con su vecina. `down = true` la baja.
    /// Devuelve false si ya esta en el extremo.
    pub fn swap_lane(&mut self, lane: usize, down: bool) -> bool {
        let pos = self.position_of(lane);
        let other = if down {
            if pos + 1 >= self.lane_order.len() {
                return false;
            }
            pos + 1
        } else {
            if pos == 0 {
                return false;
            }
            pos - 1
        };
        self.lane_order.swap(pos, other);
        true
    }
}

pub fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "4m12s", "1h02m", "31m". Compacto para caber en la tarjeta.
pub fn fmt_dur(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}
