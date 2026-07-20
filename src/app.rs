//! Estado en memoria del tablero y la logica de navegacion.

use crate::store::{now, Note, Prompt, Store, N_LANES};
use crate::tmux::{self, Pane, State};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Todo = 0,
    InProgress = 1,
    Waiting = 2,
}

impl Col {
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn from_idx(i: usize) -> Col {
        match i {
            0 => Col::Todo,
            1 => Col::InProgress,
            _ => Col::Waiting,
        }
    }
}

/// En que esta el usuario ahora mismo.
pub enum Mode {
    Board,
    /// Editor a pantalla completa de un prompt. `Some(i)` = editando el
    /// prompt i; `None` = prompt nuevo.
    PromptEdit {
        idx: Option<usize>,
        buf: String,
        cur: usize,
    },
    /// Editor de nota de un pane, dos campos.
    NoteEdit {
        pane: String,
        doing: String,
        next: String,
        /// 0 = doing, 1 = next
        field: usize,
    },
    /// Renombrar una lane.
    LaneRename {
        lane: usize,
        buf: String,
    },
    /// Confirmacion antes de escribir en un pane vivo. Deliberadamente
    /// explicita: send-keys sobre la sesion equivocada mete texto donde no toca.
    SendConfirm {
        prompt_idx: usize,
        targets: Vec<(String, String)>, // (pane_id, etiqueta)
        sel: usize,
    },
    Help,
}

pub struct App {
    pub store: Store,
    pub panes: Vec<Pane>,
    /// pane_id -> epoch en que entro en su estado actual.
    pub since: HashMap<String, u64>,
    /// pane_id -> estado en el ultimo refresco, para detectar transiciones.
    prev_state: HashMap<String, State>,
    pub col: Col,
    pub row: usize,
    pub mode: Mode,
    pub status: String,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> App {
        let mut a = App {
            store: Store::load(),
            panes: Vec::new(),
            since: HashMap::new(),
            prev_state: HashMap::new(),
            col: Col::Todo,
            row: 0,
            mode: Mode::Board,
            status: "listo".into(),
            should_quit: false,
        };
        a.refresh();
        a
    }

    /// Relee tmux y actualiza los contadores de tiempo por estado.
    ///
    /// Los tiempos no se pueden recuperar hacia atras: tmux no guarda desde
    /// cuando un pane tiene ese titulo. Los contamos desde que arranca el
    /// proceso, asi que al abrir el tablero todo empieza en 0.
    pub fn refresh(&mut self) {
        self.panes = tmux::list_claude_panes();
        let t = now();
        let mut seen = Vec::new();

        for p in &self.panes {
            seen.push(p.id.clone());
            match self.prev_state.get(&p.id) {
                Some(&old) if old == p.state => {}
                _ => {
                    self.since.insert(p.id.clone(), t);
                }
            }
            self.prev_state.insert(p.id.clone(), p.state);
        }

        self.since.retain(|k, _| seen.contains(k));
        self.prev_state.retain(|k, _| seen.contains(k));
        self.clamp();
    }

    pub fn live_ids(&self) -> Vec<String> {
        self.panes.iter().map(|p| p.id.clone()).collect()
    }

    pub fn elapsed(&self, pane_id: &str) -> Option<u64> {
        self.since.get(pane_id).map(|&s| now().saturating_sub(s))
    }

    /// Indices de los prompts que caen en una lane.
    pub fn prompts_in(&self, lane: usize) -> Vec<usize> {
        (0..self.store.prompts.len())
            .filter(|&i| self.store.prompts[i].lane == lane)
            .collect()
    }

    /// Panes de una lane y columna, ordenados por tiempo descendente: lo que
    /// lleva mas rato parado sube arriba.
    pub fn panes_in(&self, lane: usize, col: Col) -> Vec<usize> {
        let want = match col {
            Col::InProgress => State::Working,
            Col::Waiting => State::Waiting,
            Col::Todo => return Vec::new(),
        };
        let mut v: Vec<usize> = (0..self.panes.len())
            .filter(|&i| {
                self.panes[i].state == want && self.store.lane_of(&self.panes[i].id) == lane
            })
            .collect();
        v.sort_by_key(|&i| std::cmp::Reverse(self.elapsed(&self.panes[i].id).unwrap_or(0)));
        v
    }

    /// Todas las celdas de la columna activa, aplanadas lane por lane.
    /// Devuelve (lane, indice dentro de prompts/panes).
    pub fn column_cells(&self, col: Col) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for lane in 0..N_LANES {
            let items = if col == Col::Todo {
                self.prompts_in(lane)
            } else {
                self.panes_in(lane, col)
            };
            for i in items {
                out.push((lane, i));
            }
        }
        out
    }

    fn clamp(&mut self) {
        let n = self.column_cells(self.col).len();
        if n == 0 {
            self.row = 0;
        } else if self.row >= n {
            self.row = n - 1;
        }
    }

    pub fn selected(&self) -> Option<(usize, usize)> {
        self.column_cells(self.col).get(self.row).copied()
    }

    pub fn move_col(&mut self, d: i32) {
        let i = self.col.idx() as i32 + d;
        self.col = Col::from_idx(i.clamp(0, 2) as usize);
        self.row = 0;
    }

    pub fn move_row(&mut self, d: i32) {
        let n = self.column_cells(self.col).len() as i32;
        if n == 0 {
            return;
        }
        self.row = ((self.row as i32 + d).rem_euclid(n)) as usize;
    }

    /// Manda la tarjeta seleccionada a otra lane.
    pub fn assign_lane(&mut self, lane: usize) {
        let Some((_, i)) = self.selected() else { return };
        match self.col {
            Col::Todo => {
                if let Some(p) = self.store.prompts.get_mut(i) {
                    p.lane = lane;
                }
            }
            _ => {
                let id = self.panes[i].id.clone();
                self.store.pane_lane.insert(id, lane);
            }
        }
        let label = self.store.lane_label(lane);
        self.status = format!("→ {}", label);
        self.persist();
        self.clamp();
    }

    pub fn persist(&mut self) {
        let ids = self.live_ids();
        self.store.save(&ids);
    }

    // ---- acciones ----

    pub fn start_new_prompt(&mut self) {
        self.mode = Mode::PromptEdit {
            idx: None,
            buf: String::new(),
            cur: 0,
        };
    }

    pub fn start_edit(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        match self.col {
            Col::Todo => {
                let buf = self.store.prompts[i].text.clone();
                let cur = buf.len();
                self.mode = Mode::PromptEdit {
                    idx: Some(i),
                    buf,
                    cur,
                };
            }
            _ => {
                let id = self.panes[i].id.clone();
                let n = self.store.notes.get(&id).cloned().unwrap_or_default();
                self.mode = Mode::NoteEdit {
                    pane: id,
                    doing: n.doing,
                    next: n.next,
                    field: 0,
                };
            }
        }
    }

    pub fn commit_prompt(&mut self, idx: Option<usize>, buf: String) {
        let text = buf.trim_end().to_string();
        if text.trim().is_empty() {
            // Un prompt vacio no aporta nada: si estabas editando, lo borramos.
            if let Some(i) = idx {
                self.store.prompts.remove(i);
                self.status = "prompt eliminado".into();
            }
        } else {
            match idx {
                Some(i) => self.store.prompts[i].text = text,
                None => {
                    let lane = self.selected().map(|(l, _)| l).unwrap_or(0);
                    self.store.prompts.push(Prompt { text, lane });
                    self.status = "prompt guardado".into();
                }
            }
        }
        self.mode = Mode::Board;
        self.persist();
        self.clamp();
    }

    pub fn commit_note(&mut self, pane: String, doing: String, next: String) {
        if doing.trim().is_empty() && next.trim().is_empty() {
            self.store.notes.remove(&pane);
        } else {
            self.store.notes.insert(
                pane,
                Note {
                    doing: doing.trim().into(),
                    next: next.trim().into(),
                    touched: now(),
                },
            );
        }
        self.mode = Mode::Board;
        self.status = "nota guardada".into();
        self.persist();
    }

    pub fn delete_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            self.store.prompts.remove(i);
            self.status = "prompt eliminado".into();
            self.persist();
            self.clamp();
        } else {
            self.status = "solo se borran prompts del TODO".into();
        }
    }

    pub fn copy_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            tmux::copy(&self.store.prompts[i].text);
            self.status = "copiado (OSC52 + buffer tmux: prefix+])".into();
        } else {
            self.status = "nada que copiar aqui".into();
        }
    }

    /// Prepara el envio: lista de panes destino para que elijas y confirmes.
    pub fn start_send(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col != Col::Todo {
            self.status = "solo se envian prompts del TODO".into();
            return;
        }
        if self.panes.is_empty() {
            self.status = "no hay panes de Claude a los que enviar".into();
            return;
        }
        // Primero los panes de la misma lane que el prompt: son los candidatos
        // probables, y asi el destino correcto queda bajo el cursor de salida.
        let lane = self.store.prompts[i].lane;
        let mut ordered: Vec<&crate::tmux::Pane> = self.panes.iter().collect();
        ordered.sort_by_key(|p| {
            let same = self.store.lane_of(&p.id) != lane;
            // Dentro de cada grupo, los que te esperan antes que los ocupados.
            let busy = p.state == State::Working;
            (same, busy)
        });

        let targets: Vec<(String, String)> = ordered
            .iter()
            .map(|p| {
                let st = if p.state == State::Working {
                    "trabajando"
                } else {
                    "te espera"
                };
                let mark = if self.store.lane_of(&p.id) == lane {
                    "▸ "
                } else {
                    "  "
                };
                (
                    p.id.clone(),
                    format!("{}{}  {}  {} · {}", mark, p.loc, p.window, p.title, st),
                )
            })
            .collect();
        self.mode = Mode::SendConfirm {
            prompt_idx: i,
            targets,
            sel: 0,
        };
    }

    /// Escribe el prompt en el pane. NO manda Enter: lo revisas y lo lanzas tu.
    pub fn do_send(&mut self, prompt_idx: usize, pane_id: String) {
        let text = self.store.prompts[prompt_idx].text.clone();
        tmux::send_text(&pane_id, &text);

        // El prompt pasa a ser la nota "haciendo" del pane: asi la tarjeta
        // completa su viaje de TODO a la columna de estado.
        let mut n = self.store.notes.get(&pane_id).cloned().unwrap_or_default();
        let first: String = text.lines().next().unwrap_or("").chars().take(80).collect();
        n.doing = first;
        n.touched = now();
        self.store.notes.insert(pane_id.clone(), n);
        self.store.prompts.remove(prompt_idx);

        self.mode = Mode::Board;
        self.status = "escrito en el pane SIN Enter — revisalo y lanzalo tu".into();
        self.persist();
        self.clamp();
    }

    pub fn focus_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            self.status = "[s] envia el prompt a un pane".into();
        } else {
            tmux::focus_pane(&self.panes[i].id);
            self.status = format!("→ {}", self.panes[i].loc);
        }
    }

    pub fn start_rename(&mut self) {
        let lane = self.selected().map(|(l, _)| l).unwrap_or(0);
        let buf = self.store.lane_names[lane].clone();
        self.mode = Mode::LaneRename { lane, buf };
    }

    pub fn commit_rename(&mut self, lane: usize, buf: String) {
        self.store.lane_names[lane] = buf.trim().to_string();
        self.mode = Mode::Board;
        self.status = "lane renombrada".into();
        self.persist();
    }
}
