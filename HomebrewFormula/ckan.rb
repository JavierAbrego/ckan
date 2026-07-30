# Formula Homebrew de ckan. Vive en HomebrewFormula/ dentro del propio repo,
# asi que Homebrew la reconoce como un tap del repo: basta con
#   brew install JavierAbrego/ckan
# Instala el binario precompilado que publican los GitHub Releases; no compila
# desde fuente. Descarga el tarball correcto segun OS/arch y verifica su sha256.
#
# IMPORTANTE: esta formula NO funcionara hasta rellenar los 3 sha256 reales de
# mas abajo. Son placeholders textuales a proposito; brew los rechazara hasta
# que se sustituyan por el hash real de cada tarball.
class Ckan < Formula
  desc "Terminal kanban board for the Claude Code sessions across your tmux panes"
  homepage "https://github.com/JavierAbrego/ckan"
  version "0.1.0"
  license "MIT"

  # tmux es la unica dependencia: ckan solo corre dentro de una sesion tmux.
  # No se declara libc ni libgcc: en Homebrew las bibliotecas del sistema no se
  # modelan como deps de formula.
  depends_on "tmux"

  # Seleccion del artefacto por plataforma. Cada url es EXACTAMENTE la del
  # contrato de naming de los releases:
  #   https://github.com/JavierAbrego/ckan/releases/download/v0.1.0/ckan-v0.1.0-<target>.tar.gz
  #
  # Como rellenar cada sha256 tras el primer release: en la pagina del release
  # hay un fichero ckan-v0.1.0-<target>.tar.gz.sha256 junto a cada tarball.
  # Copiar aqui SOLO el hash hexadecimal (los 64 caracteres, sin el nombre de
  # fichero que a veces acompana a la salida de shasum).
  on_macos do
    if Hardware::CPU.arm?
      # macOS Apple Silicon (arm64)
      url "https://github.com/JavierAbrego/ckan/releases/download/v0.1.0/ckan-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_ckan-v0.1.0-aarch64-apple-darwin.tar.gz"
    else
      # macOS Intel (x86_64)
      url "https://github.com/JavierAbrego/ckan/releases/download/v0.1.0/ckan-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_ckan-v0.1.0-x86_64-apple-darwin.tar.gz"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      # Linux Intel (x86_64). Solo se publica este target para Linux.
      url "https://github.com/JavierAbrego/ckan/releases/download/v0.1.0/ckan-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_SHA256_OF_ckan-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
    else
      # No hay build de Linux arm64 todavia (fuera de alcance). Fallar limpio
      # en vez de servir un binario x86_64 a una maquina ARM.
      odie "ckan does not ship a Linux arm64 build yet; build from source instead."
    end
  end

  def install
    # El tarball trae un directorio ckan-v0.1.0-<target>/ con ckan + README +
    # LICENSE; Homebrew ya entra en ese directorio al extraer, asi que los
    # ficheros estan en el cwd.
    bin.install "ckan"
    doc.install "README.md" if File.exist?("README.md")
  end

  test do
    # ckan no parsea argumentos (no hay --version ni --help). Sin la variable
    # TMUX se niega a arrancar: imprime el aviso y sale con status 1.
    output = shell_output("#{bin}/ckan 2>&1", 1)
    assert_match "must run inside tmux", output
  end
end
