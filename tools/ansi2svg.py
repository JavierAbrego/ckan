#!/usr/bin/env python3
"""Convierte la salida de `tmux capture-pane -e` en un SVG con color.

Es tooling de desarrollo, no parte del binario: por eso vive en tools/ y puede
usar lo que quiera. El binario sigue con sus cuatro dependencias.

Uso:
    tmux capture-pane -t <pane> -p -e | ./ansi2svg.py salida.svg

El SVG usa <text> monoespaciado en vez de rasterizar aqui: asi el resultado
sigue siendo texto (revisable en un diff) y rsvg-convert se encarga del PNG.
"""

import re
import sys
from html import escape

# Metricas de DejaVu Sans Mono a 15px. El avance horizontal de esta fuente es
# 0.6023 em; si se cambia la fuente hay que recalcularlo o las cajas de fondo
# dejaran de cuadrar con los glifos.
FONT = "DejaVu Sans Mono"
FONT_SIZE = 15
CHAR_W = FONT_SIZE * 0.6023
LINE_H = FONT_SIZE * 1.32
PAD = 14

# Fondo del terminal y color por defecto del texto.
BG = "#1c1c22"
FG = "#d4d4dc"

# Paleta 256 -> RGB. Los 16 primeros son los colores nombrados del terminal;
# el resto se calcula (cubo 6x6x6 y rampa de grises), que es como los define
# el estandar.
BASE16 = [
    (0x1C, 0x1C, 0x22), (0xE0, 0x5A, 0x5A), (0x5F, 0xD7, 0x87), (0xD7, 0xAF, 0x5F),
    (0x5F, 0x9F, 0xE0), (0xC0, 0x7A, 0xD7), (0x5F, 0xD7, 0xD7), (0xD4, 0xD4, 0xDC),
    (0x6C, 0x6C, 0x78), (0xFF, 0x8A, 0x8A), (0x8A, 0xF0, 0xB0), (0xF0, 0xD0, 0x8A),
    (0x8A, 0xC0, 0xFF), (0xE0, 0xA0, 0xF0), (0x8A, 0xF0, 0xF0), (0xFF, 0xFF, 0xFF),
]


def xterm256(n):
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        levels = [0, 95, 135, 175, 215, 255]
        return (levels[n // 36], levels[(n // 6) % 6], levels[n % 6])
    v = 8 + (n - 232) * 10
    return (v, v, v)


def hexof(rgb):
    return "#%02x%02x%02x" % rgb


class Style:
    __slots__ = ("fg", "bg", "bold", "reverse")

    def __init__(self):
        self.fg = None
        self.bg = None
        self.bold = False
        self.reverse = False

    def copy(self):
        s = Style()
        s.fg, s.bg, s.bold, s.reverse = self.fg, self.bg, self.bold, self.reverse
        return s

    def colors(self):
        """Colores efectivos, resolviendo reverse e intensificando el bold."""
        fg = self.fg or FG
        bg = self.bg
        if self.reverse:
            # En video inverso el fondo pasa a ser el color del texto; sin un bg
            # explicito usamos el fg actual sobre el fondo del terminal.
            fg, bg = (bg or BG), (self.fg or FG)
        return fg, bg


SGR = re.compile(r"\x1b\[([0-9;]*)m")


def apply_sgr(style, params):
    """Aplica una secuencia SGR. Devuelve el estilo resultante."""
    codes = [int(p) if p else 0 for p in params.split(";")] or [0]
    i = 0
    while i < len(codes):
        c = codes[i]
        if c == 0:
            style = Style()
        elif c == 1:
            style.bold = True
        elif c == 22:
            style.bold = False
        elif c == 7:
            style.reverse = True
        elif c == 27:
            style.reverse = False
        elif c == 39:
            style.fg = None
        elif c == 49:
            style.bg = None
        elif 30 <= c <= 37:
            style.fg = hexof(BASE16[c - 30])
        elif 90 <= c <= 97:
            style.fg = hexof(BASE16[c - 90 + 8])
        elif 40 <= c <= 47:
            style.bg = hexof(BASE16[c - 40])
        elif 100 <= c <= 107:
            style.bg = hexof(BASE16[c - 100 + 8])
        elif c in (38, 48):
            # 38/48;5;N (paleta) o 38/48;2;R;G;B (color directo)
            target_fg = c == 38
            if i + 1 < len(codes) and codes[i + 1] == 5 and i + 2 < len(codes):
                col = hexof(xterm256(codes[i + 2]))
                i += 2
            elif i + 1 < len(codes) and codes[i + 1] == 2 and i + 4 < len(codes):
                col = hexof((codes[i + 2], codes[i + 3], codes[i + 4]))
                i += 4
            else:
                i += 1
                continue
            if target_fg:
                style.fg = col
            else:
                style.bg = col
        i += 1
    return style


def parse(text):
    """Trocea cada linea en spans (texto, estilo) siguiendo las secuencias SGR."""
    lines = []
    style = Style()
    for raw in text.split("\n"):
        spans = []
        pos = 0
        for m in SGR.finditer(raw):
            chunk = raw[pos:m.start()]
            if chunk:
                spans.append((chunk, style.copy()))
            style = apply_sgr(style, m.group(1))
            pos = m.end()
        tail = raw[pos:]
        if tail:
            spans.append((tail, style.copy()))
        lines.append(spans)
    return lines


def render(lines):
    # Ancho por el numero de celdas, no por len() del texto crudo: los spans ya
    # vienen sin secuencias de escape.
    cols = max((sum(len(t) for t, _ in spans) for spans in lines), default=0)
    w = int(cols * CHAR_W + PAD * 2)
    h = int(len(lines) * LINE_H + PAD * 2)

    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
        f'viewBox="0 0 {w} {h}">',
        f'<rect width="100%" height="100%" fill="{BG}" rx="6"/>',
    ]

    # Los fondos van en una capa previa para que ningun rectangulo tape el
    # texto de un span dibujado antes.
    for row, spans in enumerate(lines):
        col = 0
        y = PAD + row * LINE_H
        for text, st in spans:
            _, bg = st.colors()
            if bg:
                x = PAD + col * CHAR_W
                out.append(
                    f'<rect x="{x:.1f}" y="{y:.1f}" width="{len(text) * CHAR_W:.1f}" '
                    f'height="{LINE_H:.1f}" fill="{bg}"/>'
                )
            col += len(text)

    out.append(
        f'<g font-family="{FONT}, monospace" font-size="{FONT_SIZE}" '
        f'xml:space="preserve">'
    )
    for row, spans in enumerate(lines):
        col = 0
        # +0.78 em para asentar la linea base dentro de la caja de la fila.
        y = PAD + row * LINE_H + FONT_SIZE * 0.78
        for text, st in spans:
            fg, _ = st.colors()
            if text.strip():
                x = PAD + col * CHAR_W
                weight = ' font-weight="bold"' if st.bold else ""
                out.append(
                    f'<text x="{x:.1f}" y="{y:.1f}" fill="{fg}"{weight}>'
                    f"{escape(text)}</text>"
                )
            col += len(text)
    out.append("</g></svg>")
    return "\n".join(out)


def main():
    data = sys.stdin.read()
    # Las lineas en blanco del final solo aportan alto muerto a la imagen.
    lines = parse(data.rstrip("\n"))
    while lines and not any(t.strip() for t, _ in lines[-1]):
        lines.pop()
    svg = render(lines)
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w") as f:
            f.write(svg)
    else:
        sys.stdout.write(svg)


if __name__ == "__main__":
    main()
