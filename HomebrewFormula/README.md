# Instalar ckan con Homebrew

`ckan.rb` es la formula Homebrew de ckan. Vive dentro de este repo, en
`HomebrewFormula/`, asi que Homebrew la trata como un tap del propio repo.

## Instalacion (tap del repo)

    brew install JavierAbrego/ckan

Homebrew clona `github.com/JavierAbrego/ckan`, encuentra `HomebrewFormula/ckan.rb`
y descarga el tarball precompilado del release que corresponda a tu OS/arch
(macOS arm64, macOS Intel o Linux x86_64), verificando su sha256.

## Publicar la misma formula en un tap dedicado (opcional)

Si en algun momento existe el repo `github.com/JavierAbrego/homebrew-tap`, se
puede copiar este mismo `ckan.rb` a su carpeta `Formula/` y usar:

    brew tap JavierAbrego/tap && brew install JavierAbrego/tap/ckan

(El tap remoto NO se crea desde aqui.)

## Antes de que funcione: rellenar los sha256

La formula lleva placeholders `REPLACE_WITH_SHA256_OF_...` en los tres bloques
`sha256`. Tras el primer release (`git push` de un tag `v0.1.0`), cada tarball
del release trae un fichero acompanante `ckan-v0.1.0-<target>.tar.gz.sha256`.
Copiar el hash hexadecimal de cada uno al `sha256` correspondiente:

- aarch64-apple-darwin   -> bloque `on_macos` / `Hardware::CPU.arm?`
- x86_64-apple-darwin    -> bloque `on_macos` / else
- x86_64-unknown-linux-gnu -> bloque `on_linux`

Hasta rellenar los tres, `brew install` fallara la verificacion de checksum.

## Verificacion local hecha

- `ruby -c HomebrewFormula/ckan.rb` -> `Syntax OK`.
- Las tres URL coinciden EXACTAMENTE con el contrato de naming de los releases.
- `cargo build --release` sigue sin warnings; `src/` intacto.

## Diferido (no verificable en local, requiere el release publicado)

- `brew install` real: descarga del tarball desde GitHub Releases.
- sha256 reales: dependen de los artefactos que produzca el workflow de release.
- `brew audit` / `brew test`: requieren Homebrew instalado y el release vivo.
