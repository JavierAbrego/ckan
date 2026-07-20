# ckan

Kanban en terminal para monitorizar varias sesiones de Claude Code repartidas en
panes de tmux.

Tres columnas — **TODO** (prompts que aún no has lanzado), **IN PROGRESS** y
**WAITING** — cruzadas con seis *swimlanes* que asignas tú. Cada pane viaja de
una columna a otra dentro de su lane según el estado real de la sesión.

```
┌─ CLAUDE KANBAN · 6 sesiones ──────────────────────────────────────┐
│  📝 TODO           ⏳ IN PROGRESS        ✅ WAITING                │
│┌ 2 · INFERENCEKEY ───────────────────────────────────────────────┐│
││▏revisar los       ▏ik          0:2.3    ▏ik            0:2.1    ││
││▏constructores…    ▏custom-backends-…    ▏fix-inference-result…  ││
││                   ▏▸ sección custom     ▏· sin nota             ││
││                   ▏↳ pedir review       ▏               31m     ││
│└─────────────────────────────────────────────────────────────────┘│
└───────────────────────────────────────────────────────────────────┘
```

## Requisitos

- tmux (probado en 3.5a)
- Rust ≥ 1.75 para compilar

## Instalación

```bash
cargo build --release
install -m755 target/release/ckan ~/.local/bin/ckan
```

Ejecutar **dentro de tmux**:

```bash
ckan
# o en su propia ventana:
tmux new-window -n kanban 'ckan'
```

## Teclas

| Tecla | Acción |
|---|---|
| `←→↑↓` / `hjkl` | Navegar entre columnas y tarjetas |
| `1`–`6` | Mover la tarjeta seleccionada a esa swimlane |
| `R` | Renombrar la swimlane |
| `n` | Prompt nuevo (editor a pantalla completa) |
| `e` | Editar: prompt en TODO, nota en un pane |
| `d` | Borrar el prompt seleccionado |
| `y` | Copiar el prompt al portapapeles |
| `s` | Escribir el prompt en un pane (pide confirmación) |
| `enter` | Saltar al pane |
| `r` | Refrescar ya |
| `?` | Ayuda |
| `q` | Salir |

### Notas por pane

Cada pane admite dos campos: **▸ qué estoy haciendo** y **↳ qué espero después**.
Si una nota lleva más de una hora sin tocarse, la tarjeta lo indica (`nota 2h`)
— señal de que probablemente ya no refleja la realidad.

### Enviar un prompt

`s` sobre un prompt lista los panes de Claude como destino, poniendo primero los
de la misma swimlane (marcados `▸`) y, dentro de cada grupo, los que están
esperando antes que los ocupados.

El texto se escribe en el pane **sin pulsar Enter**: lo revisas allí y lo lanzas
tú. Es deliberado — `send-keys` sobre la sesión equivocada mete texto donde no
toca.

## Estado en disco

Lanes, prompts y notas se guardan en `~/.local/state/ckan/board.json`
(o `$XDG_STATE_HOME/ckan/`). Se anclan al `pane_id` de tmux (`%17`), no a la
posición (`0:2.1`), para que sobrevivan a reordenar ventanas. Al cerrarse un
pane, su entrada se descarta.

## Limitaciones conocidas

**La detección de estado depende del título del pane.** Claude Code escribe `✳`
cuando está idle y un spinner braille mientras trabaja. Es formato de
presentación, no una API estable: si cambia en una versión futura, la
clasificación se rompe. El patrón está aislado al principio de `src/tmux.rs`
para que corregirlo sea una línea.

**Los contadores de tiempo empiezan en cero al arrancar.** tmux no guarda desde
cuándo un pane tiene su título actual, así que las transiciones hay que
observarlas en vivo. Las notas y las lanes sí persisten.

**El portapapeles va por OSC52.** Funciona a través de SSH si tu terminal lo
soporta, con `set-clipboard on|external` en tmux. El buffer de tmux queda
siempre como respaldo (`prefix + ]`).

## Estructura

| Fichero | Contenido |
|---|---|
| `src/tmux.rs` | Diálogo con tmux; patrones de detección de estado |
| `src/store.rs` | Persistencia de lanes, prompts y notas |
| `src/app.rs` | Estado en memoria, navegación y acciones |
| `src/ui.rs` | Dibujado del tablero y overlays |
