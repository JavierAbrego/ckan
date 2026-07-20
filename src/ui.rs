//! Dibujado del tablero.

use crate::app::{App, Col, Mode};
use crate::store::{fmt_dur, now, N_LANES};
use crate::tmux::State;
use ratatui::prelude::*;
use ratatui::widgets::*;

const COL_TITLES: [&str; 3] = ["📝 TODO", "⏳ IN PROGRESS", "✅ WAITING"];

// Paleta por funcion, no por color. DarkGray desaparece casi por completo:
// sobre fondos oscuros habituales queda por debajo de un contraste legible.
//
// TEXT     texto principal, maxima legibilidad
// DIM      secundario pero que hay que poder leer (ayuda, titulos de Claude)
// FAINT    verdaderamente accesorio (ubicacion del pane, "sin nota")
const TEXT: Color = Color::White;
const DIM: Color = Color::Rgb(200, 200, 210);
const FAINT: Color = Color::Rgb(140, 140, 150);
/// Cabecera de columna inactiva: se distingue de la activa sin perderse.
const HEAD_OFF: Color = Color::Rgb(165, 165, 178);

/// Colores por lane, para que el ojo agrupe sin leer.
const LANE_COLORS: [Color; N_LANES] = [
    Color::Cyan,
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::LightRed,
];

/// Una nota mas vieja que esto ya no es de fiar: la marcamos.
const STALE_SECS: u64 = 3600;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(
                " CLAUDE KANBAN · {} sesiones ",
                app.panes.len()
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // cabecera de columnas
        Constraint::Min(0),    // swimlanes
        Constraint::Length(1), // barra de estado
    ])
    .split(inner);

    draw_header(f, rows[0], app);
    draw_lanes(f, rows[1], app);
    draw_status(f, rows[2], app);

    match &app.mode {
        Mode::PromptEdit { idx, buf, .. } => draw_prompt_editor(f, area, idx.is_some(), buf),
        Mode::NoteEdit {
            doing, next, field, ..
        } => draw_note_editor(f, area, doing, next, *field),
        Mode::LaneRename { lane, buf } => draw_rename(f, area, *lane, buf, app),
        Mode::SendConfirm { targets, sel, .. } => draw_send(f, area, targets, *sel),
        Mode::Help => draw_help(f, area),
        Mode::Board => {}
    }
}

fn col_areas(area: Rect) -> Vec<Rect> {
    Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ])
    .split(area)
    .to_vec()
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    for (i, a) in col_areas(area).iter().enumerate() {
        let active = app.col.idx() == i;
        let style = if active {
            Style::default()
                .fg(TEXT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(HEAD_OFF)
        };
        let n = app.column_cells(Col::from_idx(i)).len();
        f.render_widget(
            Paragraph::new(format!(" {} · {} ", COL_TITLES[i], n)).style(style),
            *a,
        );
    }
}

fn draw_lanes(f: &mut Frame, area: Rect, app: &App) {
    // Mostramos toda lane con contenido en CUALQUIER columna, mas la lane de
    // la seleccion actual (para poder moverte a una vacia y verla).
    // Recorremos en el orden de pintado del usuario, no por indice de lane.
    let sel_lane = app.selected().map(|(l, _)| l);
    let visible: Vec<usize> = app
        .store
        .lane_order
        .iter()
        .copied()
        .filter(|&l| {
            !app.prompts_in(l).is_empty()
                || !app.panes_in(l, Col::InProgress).is_empty()
                || !app.panes_in(l, Col::Waiting).is_empty()
                || sel_lane == Some(l)
        })
        .collect();

    if visible.is_empty() {
        f.render_widget(
            Paragraph::new("\n  Tablero vacio. [n] crea tu primer prompt.")
                .style(Style::default().fg(FAINT)),
            area,
        );
        return;
    }

    // Altura que pide cada lane segun su columna mas cargada. Usamos Length
    // (no Min) y un relleno al final: con Min, la primera lane absorbe todo el
    // espacio sobrante y empuja las demas fuera de pantalla.
    let want: Vec<u16> = visible
        .iter()
        .map(|&l| {
            let cards = app
                .prompts_in(l)
                .len()
                .max(app.panes_in(l, Col::InProgress).len())
                .max(app.panes_in(l, Col::Waiting).len());
            // ~6 lineas por tarjeta de pane (cabecera + titulo + nota + pie +
            // blanca) y 2 de borde. Suelo de 5 para que una lane vacia se vea.
            ((cards as u16 * 6) + 2).clamp(5, 26)
        })
        .collect();

    // Si no cabe todo, recortamos proporcionalmente en vez de perder lanes.
    let total: u16 = want.iter().sum();
    let avail = area.height;
    let heights: Vec<u16> = if total <= avail {
        want
    } else {
        let min_h = 5u16;
        let n = want.len() as u16;
        if avail >= min_h * n {
            // Reparto proporcional respetando el minimo.
            want.iter()
                .map(|&w| ((w as u32 * avail as u32 / total as u32) as u16).max(min_h))
                .collect()
        } else {
            // Ni con el minimo caben: reparto a partes iguales.
            vec![(avail / n).max(3); want.len()]
        }
    };

    let mut constraints: Vec<Constraint> =
        heights.iter().map(|&h| Constraint::Length(h)).collect();
    constraints.push(Constraint::Min(0)); // relleno: absorbe el sobrante

    let chunks = Layout::vertical(constraints).split(area);

    for (slot, &lane) in visible.iter().enumerate() {
        if slot >= chunks.len().saturating_sub(1) {
            break;
        }
        if chunks[slot].height >= 3 {
            draw_lane(f, chunks[slot], app, lane);
        }
    }
}

fn draw_lane(f: &mut Frame, area: Rect, app: &App, lane: usize) {
    let color = LANE_COLORS[lane];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            format!(" {} ", app.store.lane_label(lane)),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = col_areas(inner);
    for ci in 0..3 {
        let col = Col::from_idx(ci);
        let items: Vec<Line> = if col == Col::Todo {
            app.prompts_in(lane)
                .iter()
                .map(|&i| (i, ()))
                .flat_map(|(i, _)| prompt_card(app, lane, i, color, cols[ci].width))
                .collect()
        } else {
            app.panes_in(lane, col)
                .iter()
                .flat_map(|&i| pane_card(app, lane, i, col, color, cols[ci].width))
                .collect()
        };
        f.render_widget(Paragraph::new(items), cols[ci]);
    }
}

/// Marca si esta tarjeta es la seleccionada ahora mismo.
fn is_sel(app: &App, lane: usize, idx: usize, col: Col) -> bool {
    app.col == col && app.selected() == Some((lane, idx))
}

fn bar(selected: bool, color: Color) -> Span<'static> {
    if selected {
        Span::styled("▐", Style::default().fg(color).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("▏", Style::default().fg(color))
    }
}

fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(8);
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut cur = String::new();
        for word in raw.split_whitespace() {
            if cur.is_empty() {
                cur = word.to_string();
            } else if cur.chars().count() + 1 + word.chars().count() <= w {
                cur.push(' ');
                cur.push_str(word);
            } else {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn prompt_card(app: &App, lane: usize, i: usize, color: Color, w: u16) -> Vec<Line<'static>> {
    let sel = is_sel(app, lane, i, Col::Todo);
    let text = &app.store.prompts[i].text;
    let inner_w = w.saturating_sub(3) as usize;
    let lines = wrap(text, inner_w);

    let base = if sel {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };

    let mut out: Vec<Line> = lines
        .iter()
        .take(4)
        .map(|l| Line::from(vec![bar(sel, color), Span::raw(" "), Span::styled(l.clone(), base)]))
        .collect();
    if lines.len() > 4 {
        out.push(Line::from(vec![
            bar(sel, color),
            Span::styled(" …", Style::default().fg(FAINT)),
        ]));
    }
    out.push(Line::from(""));
    out
}

fn pane_card(app: &App, lane: usize, i: usize, col: Col, color: Color, w: u16) -> Vec<Line<'static>> {
    let sel = is_sel(app, lane, i, col);
    let p = &app.panes[i];
    let inner_w = w.saturating_sub(3) as usize;

    let head_style = if sel {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };

    let mut out = vec![Line::from(vec![
        bar(sel, color),
        Span::raw(" "),
        Span::styled(p.window.clone(), head_style),
        Span::styled(
            format!("  {}", p.loc),
            Style::default().fg(FAINT),
        ),
    ])];

    // Titulo que Claude se autoasigna.
    for l in wrap(&p.title, inner_w).into_iter().take(2) {
        out.push(Line::from(vec![
            bar(sel, color),
            Span::raw(" "),
            Span::styled(l, Style::default().fg(DIM)),
        ]));
    }

    // Nota del usuario: que hace y que espera despues.
    match app.store.notes.get(&p.id) {
        Some(n) if !n.doing.is_empty() || !n.next.is_empty() => {
            if !n.doing.is_empty() {
                for (k, l) in wrap(&n.doing, inner_w.saturating_sub(2)).into_iter().take(2).enumerate() {
                    out.push(Line::from(vec![
                        bar(sel, color),
                        Span::styled(
                            if k == 0 { " ▸ " } else { "   " },
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(l, Style::default().fg(TEXT)),
                    ]));
                }
            }
            if !n.next.is_empty() {
                for (k, l) in wrap(&n.next, inner_w.saturating_sub(2)).into_iter().take(2).enumerate() {
                    out.push(Line::from(vec![
                        bar(sel, color),
                        Span::styled(
                            if k == 0 { " ↳ " } else { "   " },
                            Style::default().fg(FAINT),
                        ),
                        Span::styled(l, Style::default().fg(DIM)),
                    ]));
                }
            }
        }
        _ => out.push(Line::from(vec![
            bar(sel, color),
            Span::styled(" · sin nota", Style::default().fg(FAINT)),
        ])),
    }

    // Pie: tiempo en estado + aviso de nota rancia.
    let mut foot: Vec<Span> = vec![bar(sel, color), Span::raw(" ")];
    let el = app.elapsed(&p.id);
    let long_wait = p.state == State::Waiting && el.map(|e| e >= STALE_SECS).unwrap_or(false);
    if long_wait {
        foot.push(Span::styled("⚠ ", Style::default().fg(Color::Yellow)));
    }
    foot.push(Span::styled(
        el.map(fmt_dur).unwrap_or_else(|| "--".into()),
        Style::default().fg(if long_wait {
            Color::Yellow
        } else {
            FAINT
        }),
    ));
    if let Some(n) = app.store.notes.get(&p.id) {
        let age = now().saturating_sub(n.touched);
        if age >= STALE_SECS && (!n.doing.is_empty() || !n.next.is_empty()) {
            foot.push(Span::styled(
                format!(" · nota {}", fmt_dur(age)),
                Style::default().fg(FAINT),
            ));
        }
    }
    out.push(Line::from(foot));
    out.push(Line::from(""));
    out
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let keys = match app.mode {
        Mode::Board => "[←→↑↓] nav  [J/K] mover lane  [1-6] asignar  [n]uevo [e]dit [d]el  [y] copiar  [s] enviar  [enter] saltar  [R] renombrar  [?] ayuda  [q] salir",
        _ => "[esc] cancelar",
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.status),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(format!(" {}", keys), Style::default().fg(DIM)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ---- overlays ----

fn centered(area: Rect, pw: u16, ph: u16) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - ph) / 2),
        Constraint::Percentage(ph),
        Constraint::Percentage((100 - ph) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - pw) / 2),
        Constraint::Percentage(pw),
        Constraint::Percentage((100 - pw) / 2),
    ])
    .split(v[1])[1]
}

fn draw_prompt_editor(f: &mut Frame, area: Rect, editing: bool, buf: &str) {
    let a = centered(area, 80, 70);
    f.render_widget(Clear, a);
    let title = if editing {
        " editar prompt "
    } else {
        " prompt nuevo "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " [ctrl-s] guardar · [esc] cancelar · [enter] nueva linea ",
            Style::default().fg(DIM),
        ));
    let inner = block.inner(a);
    f.render_widget(block, a);

    let text = format!("{}█", buf);
    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_note_editor(f: &mut Frame, area: Rect, doing: &str, next: &str, field: usize) {
    let a = centered(area, 70, 40);
    f.render_widget(Clear, a);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " nota del pane ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " [tab] cambiar campo · [ctrl-s] guardar · [esc] cancelar ",
            Style::default().fg(DIM),
        ));
    let inner = block.inner(a);
    f.render_widget(block, a);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(inner);

    let lbl = |s: &'static str, on: bool| {
        Paragraph::new(s).style(if on {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(FAINT)
        })
    };
    f.render_widget(lbl("▸ que estoy haciendo", field == 0), rows[0]);
    f.render_widget(
        Paragraph::new(if field == 0 {
            format!("{}█", doing)
        } else {
            doing.to_string()
        })
        .wrap(Wrap { trim: false }),
        rows[1],
    );
    f.render_widget(lbl("↳ que espero despues", field == 1), rows[2]);
    f.render_widget(
        Paragraph::new(if field == 1 {
            format!("{}█", next)
        } else {
            next.to_string()
        })
        .wrap(Wrap { trim: false }),
        rows[3],
    );
}

fn draw_rename(f: &mut Frame, area: Rect, lane: usize, buf: &str, app: &App) {
    let a = centered(area, 50, 20);
    f.render_widget(Clear, a);
    let color = LANE_COLORS[lane];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            format!(" renombrar lane {} ", lane + 1),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " [enter] guardar · [esc] cancelar ",
            Style::default().fg(DIM),
        ));
    let inner = block.inner(a);
    f.render_widget(block, a);
    let _ = app;
    f.render_widget(Paragraph::new(format!("{}█", buf)), inner);
}

fn draw_send(f: &mut Frame, area: Rect, targets: &[(String, String)], sel: usize) {
    let a = centered(area, 75, 60);
    f.render_widget(Clear, a);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            " ¿a que pane lo escribo? ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " [↑↓] elegir · [enter] escribir y saltar · [esc] cancelar ",
            Style::default().fg(DIM),
        ));
    let inner = block.inner(a);
    f.render_widget(block, a);

    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);
    f.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled("▸ ", Style::default().fg(TEXT)),
                Span::styled(
                    "= misma swimlane que el prompt",
                    Style::default().fg(DIM),
                ),
            ]),
            Line::from(Span::styled(
                "Se escribe en el pane y saltamos alli: revisa y pulsa Enter.",
                Style::default().fg(DIM),
            )),
        ])),
        rows[0],
    );

    // Los de la lane del prompt vienen marcados con "▸" y ordenados primero.
    let items: Vec<ListItem> = targets
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let same_lane = label.starts_with('▸');
            let st = if i == sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if same_lane {
                Style::default().fg(TEXT)
            } else {
                Style::default().fg(FAINT)
            };
            ListItem::new(format!(" {} ", label)).style(st)
        })
        .collect();
    f.render_widget(List::new(items), rows[1]);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let a = centered(area, 62, 75);
    f.render_widget(Clear, a);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(Span::styled(
            " ayuda ",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " [esc] cerrar ",
            Style::default().fg(DIM),
        ));
    let inner = block.inner(a);
    f.render_widget(block, a);

    let t = "\
 NAVEGAR
   ←  →        cambiar de columna
   ↑  ↓        moverse entre tarjetas
   enter       saltar al pane (IN PROGRESS / WAITING)

 ORGANIZAR
   1 … 6       mover la tarjeta a esa swimlane
   R           renombrar la swimlane de la seleccion
   shift+J     bajar la swimlane entera una posicion
   shift+K     subirla (tambien shift+↓ / shift+↑)

   Reordenar cambia solo donde se pinta la lane: su numero
   no cambia, asi que la tecla que la selecciona sigue
   siendo la misma.

 PROMPTS (columna TODO)
   n           prompt nuevo (editor a pantalla completa)
   e           editar el prompt seleccionado
   d           borrar el prompt seleccionado
   y           copiar al portapapeles
   s           escribir el prompt en un pane y saltar alli.
               Pide confirmacion del destino y NO manda Enter:
               llegas al pane con el texto puesto, revisas y
               pulsas Enter para lanzarlo.

 PANES
   e           editar la nota: que haces / que esperas despues

 OTROS
   r           refrescar ya
   q           salir

 NOTAS
   El estado (trabajando / te espera) se detecta por el titulo
   que Claude Code escribe en el pane. Si una version futura
   cambia ese formato, la clasificacion se rompe: el patron
   esta aislado en src/tmux.rs.

   Los tiempos se cuentan desde que arranca el tablero: tmux no
   guarda desde cuando un pane esta en su estado, asi que al
   abrir empiezan todos en cero.";
    f.render_widget(Paragraph::new(t), inner);
}
