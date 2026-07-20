mod app;
mod store;
mod tmux;
mod ui;

use app::{App, Mode};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use std::io;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(250);
const REFRESH: Duration = Duration::from_secs(2);

fn main() -> io::Result<()> {
    if std::env::var("TMUX").is_err() {
        eprintln!("ckan hay que ejecutarlo dentro de tmux.");
        std::process::exit(1);
    }

    // $TMUX puede venir heredada en un shell sin terminal real (scripts, CI).
    // Sin esta comprobacion, enable_raw_mode falla con un error de sistema
    // criptico en vez de decir lo que pasa.
    if !std::io::IsTerminal::is_terminal(&io::stdout()) {
        eprintln!("ckan necesita un terminal interactivo (stdout no lo es).");
        std::process::exit(1);
    }

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let mut term = Terminal::new(CrosstermBackend::new(out))?;

    let res = run(&mut term);

    disable_raw_mode()?;
    execute!(
        term.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    term.show_cursor()?;
    res
}

fn run<B: Backend>(term: &mut Terminal<B>) -> io::Result<()> {
    let mut app = App::new();
    let mut last = Instant::now();

    loop {
        term.draw(|f| ui::draw(f, &app))?;

        if event::poll(TICK)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    on_key(&mut app, k);
                }
            }
        }

        if last.elapsed() >= REFRESH {
            app.refresh();
            last = Instant::now();
        }

        if app.should_quit {
            app.persist();
            return Ok(());
        }
    }
}

fn on_key(app: &mut App, k: KeyEvent) {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

    // Tomamos el modo por valor para poder mutar `app` dentro de cada rama.
    let mode = std::mem::replace(&mut app.mode, Mode::Board);

    match mode {
        Mode::Board => {
            app.mode = Mode::Board;
            on_board_key(app, k, ctrl);
        }

        Mode::PromptEdit {
            idx,
            mut buf,
            mut cur,
        } => match k.code {
            KeyCode::Esc => app.mode = Mode::Board,
            KeyCode::Char('s') if ctrl => app.commit_prompt(idx, buf),
            KeyCode::Enter => {
                buf.insert(cur, '\n');
                cur += 1;
                app.mode = Mode::PromptEdit { idx, buf, cur };
            }
            KeyCode::Backspace => {
                if cur > 0 {
                    // Retrocedemos hasta el limite de caracter anterior para no
                    // partir un multibyte por la mitad.
                    let mut p = cur - 1;
                    while p > 0 && !buf.is_char_boundary(p) {
                        p -= 1;
                    }
                    buf.remove(p);
                    cur = p;
                }
                app.mode = Mode::PromptEdit { idx, buf, cur };
            }
            KeyCode::Char(c) => {
                buf.insert(cur, c);
                cur += c.len_utf8();
                app.mode = Mode::PromptEdit { idx, buf, cur };
            }
            _ => app.mode = Mode::PromptEdit { idx, buf, cur },
        },

        Mode::NoteEdit {
            pane,
            mut doing,
            mut next,
            mut field,
        } => {
            let done = match k.code {
                KeyCode::Esc => {
                    app.mode = Mode::Board;
                    true
                }
                KeyCode::Char('s') if ctrl => {
                    app.commit_note(pane.clone(), doing.clone(), next.clone());
                    true
                }
                KeyCode::Enter => {
                    app.commit_note(pane.clone(), doing.clone(), next.clone());
                    true
                }
                KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                    field = 1 - field;
                    false
                }
                KeyCode::Backspace => {
                    let b = if field == 0 { &mut doing } else { &mut next };
                    b.pop();
                    false
                }
                KeyCode::Char(c) => {
                    let b = if field == 0 { &mut doing } else { &mut next };
                    b.push(c);
                    false
                }
                _ => false,
            };
            if !done {
                app.mode = Mode::NoteEdit {
                    pane,
                    doing,
                    next,
                    field,
                };
            }
        }

        Mode::LaneRename { lane, mut buf } => match k.code {
            KeyCode::Esc => app.mode = Mode::Board,
            KeyCode::Enter => app.commit_rename(lane, buf),
            KeyCode::Backspace => {
                buf.pop();
                app.mode = Mode::LaneRename { lane, buf };
            }
            KeyCode::Char(c) => {
                buf.push(c);
                app.mode = Mode::LaneRename { lane, buf };
            }
            _ => app.mode = Mode::LaneRename { lane, buf },
        },

        Mode::SendConfirm {
            prompt_idx,
            targets,
            mut sel,
        } => match k.code {
            KeyCode::Esc => {
                app.mode = Mode::Board;
                app.status = "envio cancelado".into();
            }
            KeyCode::Up => {
                sel = sel.saturating_sub(1);
                app.mode = Mode::SendConfirm {
                    prompt_idx,
                    targets,
                    sel,
                };
            }
            KeyCode::Down => {
                sel = (sel + 1).min(targets.len().saturating_sub(1));
                app.mode = Mode::SendConfirm {
                    prompt_idx,
                    targets,
                    sel,
                };
            }
            KeyCode::Enter => {
                if let Some((id, _)) = targets.get(sel) {
                    app.do_send(prompt_idx, id.clone());
                } else {
                    app.mode = Mode::Board;
                }
            }
            _ => {
                app.mode = Mode::SendConfirm {
                    prompt_idx,
                    targets,
                    sel,
                }
            }
        },

        Mode::Help => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                app.mode = Mode::Board;
            } else {
                app.mode = Mode::Help;
            }
        }
    }
}

fn on_board_key(app: &mut App, k: KeyEvent, _ctrl: bool) {
    match k.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Left | KeyCode::Char('h') => app.move_col(-1),
        KeyCode::Right | KeyCode::Char('l') => app.move_col(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_row(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_row(1),
        KeyCode::Char(c @ '1'..='6') => {
            let lane = c.to_digit(10).unwrap() as usize - 1;
            app.assign_lane(lane);
        }
        KeyCode::Char('n') => app.start_new_prompt(),
        KeyCode::Char('e') => app.start_edit(),
        KeyCode::Char('d') => app.delete_selected(),
        KeyCode::Char('y') => app.copy_selected(),
        KeyCode::Char('s') => app.start_send(),
        KeyCode::Char('R') => app.start_rename(),
        KeyCode::Char('r') => {
            app.refresh();
            app.status = "refrescado".into();
        }
        KeyCode::Enter => app.focus_selected(),
        _ => {}
    }
}
