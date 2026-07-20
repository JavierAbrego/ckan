#!/usr/bin/env bash
# Regenera los screenshots del README desde el render real de la TUI.
#
#   ./tools/screenshots.sh
#
# Monta un escenario de demostracion inventado y captura la pantalla con sus
# colores para convertirla a PNG.
#
# Dos invariantes que este script NO puede romper:
#
#   1. No muestra las sesiones reales del usuario. `ckan` usa `list-panes -a`,
#      que recorre TODAS las sesiones del servidor tmux, asi que no basta con
#      abrir una ventana aparte: hay que levantar un SERVIDOR tmux propio con
#      su propio socket (-L). Ahi dentro solo existen los panes de demo.
#   2. No toca ~/.local/state/ckan/board.json. Apunta XDG_STATE_HOME a un
#      directorio temporal, que es lo que store.rs consulta primero.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO=$PWD
OUT=$REPO/docs/img
BIN=$REPO/target/release/ckan

# Socket propio: aisla por completo este servidor del que usa el usuario.
SOCK=ckan-shots-$$
T="tmux -L $SOCK"

command -v tmux >/dev/null || { echo "hace falta tmux" >&2; exit 1; }
command -v rsvg-convert >/dev/null || { echo "hace falta rsvg-convert (librsvg2-bin)" >&2; exit 1; }
command -v cc >/dev/null || { echo "hace falta un compilador C (cc) para los panes de demo" >&2; exit 1; }
command -v python3 >/dev/null || { echo "hace falta python3" >&2; exit 1; }

[ -x "$BIN" ] || cargo build --release
mkdir -p "$OUT"

STATE=$(mktemp -d)
cleanup() {
  $T kill-server 2>/dev/null || true
  rm -rf "$STATE"
}
trap cleanup EXIT

# --- Binario de demo -------------------------------------------------------
# ckan solo lista panes cuyo pane_current_command sea exactamente `claude`.
# Para la demo compilamos un binario trivial CON ESE NOMBRE.
#
# Por que un binario y no un script: `exec -a claude sleep` no vale (coreutils
# valida argv[0] contra el ejecutable real y aborta con "Security violation"),
# y un script con shebang reporta `bash` como comando, no `claude`. Un binario
# propio es la unica via que tmux reporta como `claude` sin falsear nada.
FAKE=$STATE/bin
mkdir -p "$FAKE"
cc -o "$FAKE/claude" -x c - <<'EOF'
#include <unistd.h>
int main(void){ for(;;) pause(); return 0; }
EOF

# --- Servidor y panes de demostracion --------------------------------------
WORKING=$'⠐'   # spinner braille -> IN PROGRESS
IDLE=$'✳'      # marca idle      -> WAITING

# El servidor tiene que existir antes de crear nada. La ventana inicial corre
# un `sleep`, no el binario de demo: si corriese `claude` se contaria como una
# sesion mas y apareceria en el tablero.
# 48 filas, no 40: el overlay de ayuda ocupa el 75% del alto y su contenido son
# 44 lineas, asi que por debajo de ~46 filas la ultima seccion se recorta y el
# screenshot saldria incompleto. (El recorte es preexistente y esta anotado como
# deuda de layout en .manager/memory/decision-anchura-80-columnas.md.)
$T new-session -d -s demo -x 132 -y 48 -c "$REPO" "sleep 86400"

# La tarjeta muestra el nombre de la VENTANA, no el del comando: sin -n todas
# saldrian como "claude" y no se distinguiria un proyecto de otro.
demo_pane() { # <ventana> <marca> <titulo>
  local id
  id=$($T new-window -d -P -F '#{pane_id}' -n "$1" "$FAKE/claude")
  $T select-pane -t "$id" -T "$2 $3"
  echo "$id"
}

P1=$(demo_pane auth     "$IDLE"    "refactor-auth-middleware")
P2=$(demo_pane ui       "$IDLE"    "add-pagination-to-list")
P3=$(demo_pane db       "$WORKING" "migrate-schema-v3")
P4=$(demo_pane ui       "$IDLE"    "fix-flaky-upload-test")
P5=$(demo_pane deploy   "$WORKING" "write-deploy-runbook")
P6=$(demo_pane docs     "$IDLE"    "document-webhook-retries")

# --- Estado de demostracion ------------------------------------------------
# Nombres genericos a proposito: el repo es publico y no debe filtrar los
# proyectos reales del usuario.
#
# `touched` va con marcas recientes calculadas sobre la hora actual: un valor
# fijo (p. ej. 1) daria una antiguedad de decadas y pintaria "note 495714h".
NOW=$(date +%s)
mkdir -p "$STATE/ckan"
cat > "$STATE/ckan/board.json" <<EOF
{
  "lane_names": ["api", "web", "infra", "", "", ""],
  "lane_order": [0, 1, 2, 3, 4, 5],
  "prompts": [
    { "text": "Add rate limiting to the public endpoints, 100 req/min per API key.", "lane": 0 },
    { "text": "Audit the Dockerfile for unpinned base images.", "lane": 2 }
  ],
  "pane_lane": {
    "$P1": 0, "$P2": 1, "$P3": 0,
    "$P4": 1, "$P5": 2, "$P6": 2
  },
  "notes": {
    "$P1": {
      "doing": "splitting the token check out of the router",
      "next": "run the integration suite before merging",
      "touched": $((NOW - 240))
    },
    "$P3": {
      "doing": "backfilling the new columns",
      "next": "verify row counts match",
      "touched": $((NOW - 90))
    },
    "$P5": {
      "doing": "drafting the rollback section",
      "next": "have someone dry-run it on staging",
      "touched": $((NOW - 5400))
    }
  }
}
EOF

# --- Captura ---------------------------------------------------------------
$T new-window -d -n board -c "$REPO" "XDG_STATE_HOME=$STATE $BIN"
sleep 2.5

shot() { # <nombre> [tecla...]
  local name=$1; shift
  for k in "$@"; do $T send-keys -t board "$k"; sleep 0.45; done
  sleep 0.5
  $T capture-pane -t board -p -e \
    | python3 "$REPO/tools/ansi2svg.py" "$OUT/$name.svg"
  rsvg-convert "$OUT/$name.svg" -o "$OUT/$name.png"
  rm -f "$OUT/$name.svg"
  echo "  $OUT/$name.png"
}

echo "generando:"
shot board                        # tablero
shot help        '?'              # ayuda
shot prompt-edit Escape 'e'       # editor de prompt (arranca en TODO)
shot send        Escape 's'       # dialogo de envio
$T send-keys -t board Escape; sleep 0.3

echo "listo."
