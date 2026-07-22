//! Estado en memoria del tablero y la logica de navegacion.

use crate::store::{now, Note, Prompt, Store};
use crate::tmux::{self, Pane, State};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Todo = 0,
    InProgress = 1,
    Waiting = 2,
}

/// Posicion visual de una tarjeta dentro de la seccion activa, usada por la
/// navegacion 2D (←→ entre sub-columnas, ↑↓ dentro de una sub-columna).
#[derive(Debug, Clone, Copy)]
struct Cell {
    /// Indice lineal en column_cells (lo que guarda `App::row`).
    row: usize,
    /// Posicion de la lane en el orden de pintado.
    lane_pos: usize,
    /// Sub-columna: 0 = izquierda, 1 = derecha.
    subcol: usize,
    /// Fila dentro de la sub-columna de esa lane.
    vrow: usize,
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
    /// Sub-columnas por seccion en el ultimo pintado (1 o 2). Lo fija el
    /// dibujado segun el ancho de pantalla; la navegacion ←→ lo consulta para
    /// moverse entre sub-columnas antes de saltar de seccion.
    pub section_cols: usize,
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
            status: "ready".into(),
            should_quit: false,
            section_cols: 1,
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
        // En el orden de pintado, para que ↑↓ siga lo que se ve en pantalla.
        for lane in self.store.lane_order.clone() {
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

    /// Geometria visual de la seccion activa: una celda por tarjeta con su
    /// indice lineal (`row` en column_cells) y su posicion en pantalla
    /// (lane en orden de pintado, sub-columna 0/1, y fila dentro de la
    /// sub-columna). Reproduce el reparto del dibujado: dentro de cada lane la
    /// primera mitad de las tarjetas (ceil) va a la sub-columna izquierda y el
    /// resto a la derecha, cuando hay dos sub-columnas.
    fn grid(&self) -> Vec<Cell> {
        let ncol = self.section_cols.max(1);
        let mut out = Vec::new();
        let mut row = 0usize; // indice lineal, en el mismo orden que column_cells
        for (lane_pos, lane) in self.store.lane_order.iter().copied().enumerate() {
            let items = if self.col == Col::Todo {
                self.prompts_in(lane)
            } else {
                self.panes_in(lane, self.col)
            };
            let n = items.len();
            // Corte izquierda/derecha, igual que el dibujado.
            let split = if ncol >= 2 { n.div_ceil(2) } else { n };
            for (k, _) in items.iter().enumerate() {
                let (subcol, vrow) = if k < split {
                    (0usize, k)
                } else {
                    (1usize, k - split)
                };
                out.push(Cell {
                    row,
                    lane_pos,
                    subcol,
                    vrow,
                });
                row += 1;
            }
        }
        out
    }

    pub fn move_col(&mut self, d: i32) {
        // Primero intentamos movernos entre sub-columnas de la misma seccion;
        // solo si ya estamos en el borde saltamos a la seccion contigua.
        let grid = self.grid();
        if let Some(cur) = grid.iter().find(|c| c.row == self.row).copied() {
            let target_sub = cur.subcol as i32 + d;
            if target_sub >= 0 {
                // Buscamos en la MISMA lane la sub-columna destino, a la fila
                // visual mas cercana. Si existe, nos quedamos en esta seccion.
                let candidates: Vec<&Cell> = grid
                    .iter()
                    .filter(|c| c.lane_pos == cur.lane_pos && c.subcol as i32 == target_sub)
                    .collect();
                if let Some(best) = candidates
                    .iter()
                    .min_by_key(|c| (c.vrow as i32 - cur.vrow as i32).abs())
                {
                    self.row = best.row;
                    return;
                }
            }
        }
        // Borde de la seccion: cambiamos de columna (TODO/IN PROGRESS/WAITING).
        let i = self.col.idx() as i32 + d;
        let new = Col::from_idx(i.clamp(0, 2) as usize);
        if new != self.col {
            self.col = new;
            self.row = 0;
        }
    }

    pub fn move_row(&mut self, d: i32) {
        // ↑↓ se mueve dentro de la sub-columna actual. Con una sola sub-columna
        // esto es el recorrido lineal de siempre.
        let grid = self.grid();
        if grid.is_empty() {
            return;
        }
        let Some(cur) = grid.iter().find(|c| c.row == self.row).copied() else {
            return;
        };
        // Celdas de la misma sub-columna (atravesando lanes), en orden de
        // pintado. Nos movemos por esa lista con envoltura.
        let column: Vec<&Cell> = grid.iter().filter(|c| c.subcol == cur.subcol).collect();
        if let Some(pos) = column.iter().position(|c| c.row == self.row) {
            let np = ((pos as i32 + d).rem_euclid(column.len() as i32)) as usize;
            self.row = column[np].row;
        } else if d != 0 {
            self.row = ((self.row as i32 + d).rem_euclid(grid.len() as i32)) as usize;
        }
    }

    /// Manda la tarjeta seleccionada a la lane que ocupa esa POSICION.
    /// La tecla 1 siempre lleva a la lane de arriba, la 2 a la siguiente, etc.,
    /// independientemente de como se hayan reordenado.
    pub fn assign_lane(&mut self, pos: usize) {
        let lane = self.store.lane_at_position(pos);
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

    /// Mueve la lane de la seleccion una posicion arriba/abajo, con todo su
    /// contenido (nombre, prompts, panes). No cambia el indice de la lane, asi
    /// que las teclas 1-9 siguen llevando a la misma lane de siempre.
    pub fn move_lane(&mut self, down: bool) {
        let Some((lane, item)) = self.selected() else {
            self.status = "nothing selected".into();
            return;
        };
        if !self.store.swap_lane(lane, down) {
            self.status = if down {
                "already the last swimlane".into()
            } else {
                "already the first swimlane".into()
            };
            return;
        }

        // La selecion debe seguir a la tarjeta, no quedarse en la misma fila:
        // tras mover la lane, esa fila muestra contenido de otra lane.
        if let Some(row) = self
            .column_cells(self.col)
            .iter()
            .position(|&(l, i)| l == lane && i == item)
        {
            self.row = row;
        }

        let label = self.store.lane_label(lane);
        self.status = format!("{} → position {}", label, self.store.position_of(lane) + 1);
        self.persist();
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
                self.status = "prompt deleted".into();
            }
        } else {
            match idx {
                Some(i) => self.store.prompts[i].text = text,
                None => {
                    let lane = self.selected().map(|(l, _)| l).unwrap_or(0);
                    self.store.prompts.push(Prompt { text, lane });
                    self.status = "prompt saved".into();
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
        self.status = "note saved".into();
        self.persist();
    }

    pub fn delete_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            self.store.prompts.remove(i);
            self.status = "prompt deleted".into();
            self.persist();
            self.clamp();
        } else {
            self.status = "only TODO prompts can be deleted".into();
        }
    }

    pub fn copy_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            tmux::copy(&self.store.prompts[i].text);
            self.status = "copied (OSC52 + tmux buffer: prefix+])".into();
        } else {
            self.status = "nothing to copy here".into();
        }
    }

    /// Prepara el envio: lista de panes destino para que elijas y confirmes.
    pub fn start_send(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col != Col::Todo {
            self.status = "only TODO prompts can be sent".into();
            return;
        }
        if self.panes.is_empty() {
            self.status = "no Claude panes to send to".into();
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
                    "working"
                } else {
                    "waiting for you"
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
        self.status = "typed into the pane — press Enter there to run it".into();
        // Persistimos ANTES de saltar: el foco se va a otro pane y no volvemos
        // a pasar por aqui.
        self.persist();
        self.clamp();

        // Saltamos al pane destino para que baste con pulsar Enter.
        tmux::focus_pane(&pane_id);
    }

    pub fn focus_selected(&mut self) {
        let Some((_, i)) = self.selected() else { return };
        if self.col == Col::Todo {
            self.status = "[s] sends the prompt to a pane".into();
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
        self.status = "swimlane renamed".into();
        self.persist();
    }
}
