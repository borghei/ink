# Installing ink

ink is a single binary with no runtime dependencies. Pick whichever route you already use.

- [Install script (macOS, Linux)](#install-script-macos-linux)
- [Install script (Windows)](#install-script-windows)
- [Homebrew](#homebrew)
- [Cargo](#cargo)
- [Scoop](#scoop)
- [Conda and Pixi](#conda-and-pixi)
- [mise](#mise)
- [.deb and .rpm packages](#deb-and-rpm-packages)
- [Prebuilt binaries](#prebuilt-binaries)
- [From source](#from-source)
- [After installing](#after-installing)

## Install script (macOS, Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | sh
```

The script picks the right binary for your OS, CPU and libc. Alpine, Raspberry Pi and systems with an older glibc get the static musl build. It checks the download against `SHA256SUMS` and installs to `/usr/local/bin`, using `sudo` only when it has to.

Two environment variables change what it does:

| Variable | Effect |
|---|---|
| `INK_VERSION` | Install a specific release, e.g. `v0.11.1`. Default: latest. |
| `INK_INSTALL_DIR` | Install somewhere else, e.g. `$HOME/.local/bin`. |

```bash
curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | INK_VERSION=v0.11.1 INK_INSTALL_DIR="$HOME/.local/bin" sh
```

## Install script (Windows)

```powershell
irm https://raw.githubusercontent.com/borghei/ink/main/install.ps1 | iex
```

This installs `ink.exe` (x64 or ARM64) to `%LOCALAPPDATA%\Programs\ink` and adds it to your user `PATH`. Set `$env:INK_VERSION = "v0.11.1"` first to pin a version.

## Homebrew

macOS and Linux:

```bash
brew tap borghei/tap
brew trust borghei/tap   # recent Homebrew wants third-party taps trusted
brew install ink
```

## Cargo

```bash
cargo install ink-md        # build from source
cargo binstall ink-md       # or download the prebuilt binary (needs cargo-binstall)
```

The crate is called `ink-md`. The binary it installs is `ink`.

## Scoop

```powershell
scoop bucket add borghei https://github.com/borghei/scoop-bucket
scoop install ink
```

## Conda and Pixi

The [conda-forge package](https://anaconda.org/conda-forge/ink-md) is maintained by the community.

```bash
conda install -c conda-forge ink-md
pixi global install ink-md
```

## mise

```bash
mise use -g cargo:ink-md    # through crates.io
mise use -g ubi:borghei/ink # straight from GitHub releases
```

## .deb and .rpm packages

Download the package from the [releases page](https://github.com/borghei/ink/releases), then:

```bash
sudo apt install ./ink-md_*_amd64.deb    # Debian, Ubuntu
sudo dnf install ./ink-md-*.x86_64.rpm   # Fedora, openSUSE
```

The packages install the man page and shell completions too.

## Prebuilt binaries

Every release on the [releases page](https://github.com/borghei/ink/releases) has these:

| Platform | Asset |
|---|---|
| Linux x86_64, arm64 (glibc 2.35+: Ubuntu 22.04, Debian 12 and newer) | `ink-linux-amd64`, `ink-linux-arm64` |
| Linux x86_64, arm64, armv7, static (any distro: Alpine, Debian 11, Ubuntu 20.04, RHEL 8, NixOS, containers, Raspberry Pi) | `ink-linux-amd64-musl`, `ink-linux-arm64-musl`, `ink-linux-armv7-musl` |
| macOS Intel, Apple Silicon | `ink-macos-amd64`, `ink-macos-arm64` |
| Windows x64, ARM64 | `ink-windows-amd64.exe`, `ink-windows-arm64.exe` |

On Linux, the glibc builds need glibc 2.35 or newer. Anything older, or musl-based, should use the static builds. The install script makes that choice for you.

The Windows builds link the C runtime statically, so you don't need the Visual C++ redistributable.

Each binary also ships as an archive (`ink-<version>-<os>-<arch>.tar.gz`, or `.zip` on Windows) with the man page and bash, zsh and fish completions inside.

### Verifying a download

`SHA256SUMS` is published with every release. Each asset also carries a signed build provenance attestation, which the GitHub CLI can check:

```bash
gh attestation verify ink-linux-amd64 --repo borghei/ink
```

The static musl builds and the Windows ARM64 binary started with v0.9.0. Older releases only have the glibc, macOS and Windows x64 builds.

## From source

You need a recent stable Rust toolchain.

```bash
git clone https://github.com/borghei/ink.git
cd ink
cargo build --release
./target/release/ink --version
```

## After installing

```bash
ink --version
ink config init                              # write a commented starter config
ink completions zsh > ~/.zfunc/_ink          # bash, zsh, fish, powershell, elvish
ink man > /usr/local/share/man/man1/ink.1    # man page
```

Next: [Features](features.md) or [Configuration](configuration.md).
