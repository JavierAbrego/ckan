//! Todo lo que sabe hablar con tmux.
//!
//! AVISO: la deteccion de estado se apoya en el titulo que Claude Code escribe
//! en el pane. Es formato de presentacion, no una API estable: si cambia en una
//! version futura, todo cae a la columna equivocada. Los patrones estan
//! aislados aqui abajo para que retocarlos sea una linea.

use std::process::Command;

/// Prefijo que Claude Code pone cuando esta idle esperando input.
const IDLE_MARK: char = '\u{2733}'; // ✳
/// Caracteres de spinner braille que usa mientras trabaja.
const BUSY_MARKS: &[char] = &[
    '\u{2801}', '\u{2802}', '\u{2804}', '\u{2808}', '\u{2810}', '\u{2820}', '\u{2840}', '\u{2880}',
    '\u{2807}', '\u{280b}', '\u{280d}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}',
    '\u{2826}', '\u{2827}', '\u{2807}', '\u{280f}', '\u{2810}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Working,
    Waiting,
}

#[derive(Debug, Clone)]
pub struct Pane {
    /// Identificador estable de tmux (%17). Sobrevive a renumeraciones.
    pub id: String,
    /// Posicion legible (0:2.1). Cambia si reordenas la ventana.
    pub loc: String,
    pub window: String,
    pub title: String,
    pub state: State,
}

fn tmux(args: &[&str]) -> Option<String> {
    let out = Command::new("tmux").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lee todos los panes que estan corriendo Claude y los clasifica.
pub fn list_claude_panes() -> Vec<Pane> {
    let fmt = "#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t#{window_name}\t#{pane_current_command}\t#{pane_title}";
    let raw = match tmux(&["list-panes", "-a", "-F", fmt]) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut panes = Vec::new();
    for line in raw.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let (id, loc, window, cmd, title) = (f[0], f[1], f[2], f[3], f[4]);
        if cmd != "claude" {
            continue;
        }

        let first = title.chars().next().unwrap_or(' ');
        let state = if BUSY_MARKS.contains(&first) {
            State::Working
        } else if first == IDLE_MARK {
            State::Waiting
        } else {
            // Titulo sin marca reconocible. Lo tratamos como que te espera:
            // preferimos un falso "revisa esto" a esconder trabajo parado.
            State::Waiting
        };

        // Quitamos la marca y el espacio para quedarnos con el nombre limpio.
        let clean = title
            .chars()
            .skip(1)
            .collect::<String>()
            .trim()
            .to_string();

        panes.push(Pane {
            id: id.to_string(),
            loc: loc.to_string(),
            window: window.to_string(),
            title: if clean.is_empty() {
                title.to_string()
            } else {
                clean
            },
            state,
        });
    }
    panes
}

/// Salta el foco de tmux al pane indicado.
pub fn focus_pane(pane_id: &str) {
    let _ = tmux(&["switch-client", "-t", pane_id]);
    let _ = tmux(&["select-pane", "-t", pane_id]);
}

/// Escribe texto en un pane SIN mandar Enter. El usuario revisa y lanza.
/// Usamos `-l` (literal) para que el texto no se interprete como nombres de tecla.
pub fn send_text(pane_id: &str, text: &str) {
    let _ = tmux(&["send-keys", "-t", pane_id, "-l", text]);
}

/// Copia al portapapeles. En esta VM no hay X11, asi que la via real es OSC52
/// (set-clipboard external + allow-passthrough on, ya verificados).
/// El buffer de tmux queda siempre como red de seguridad: prefix+] pega.
pub fn copy(text: &str) {
    use std::io::Write;
    use std::process::Stdio;

    if let Ok(mut ch) = Command::new("tmux")
        .args(["load-buffer", "-w", "-"])
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(mut si) = ch.stdin.take() {
            let _ = si.write_all(text.as_bytes());
        }
        let _ = ch.wait();
    }
}
