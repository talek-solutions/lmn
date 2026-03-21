# Installation

## Homebrew (macOS)

!!! note "Work in progress"
    Homebrew support is coming soon.

## Debian / Ubuntu

!!! note "Work in progress"
    `.deb` packages are coming soon.

## cargo install

The simplest way to install lmn if you have the Rust toolchain:

```bash
cargo install lmn
```

## Pre-built Binaries

Download a pre-built binary for your platform from the [latest GitHub release](https://github.com/talek-solutions/lmn/releases/latest):

| Platform | File |
|---|---|
| Linux x86_64 | `lmn-v{{ version }}-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `lmn-v{{ version }}-aarch64-unknown-linux-gnu.tar.gz` |
| macOS ARM64 (Apple Silicon) | `lmn-v{{ version }}-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `lmn-v{{ version }}-x86_64-pc-windows-msvc.zip` |

```bash
# Example for Linux x86_64
tar -xzf lmn-v{{ version }}-x86_64-unknown-linux-gnu.tar.gz
sudo mv lmn /usr/local/bin/
```

## Docker

```bash
docker pull ghcr.io/talek-solutions/lmn:latest
docker run --rm ghcr.io/talek-solutions/lmn:latest run -H https://httpbin.org/get
```

## From Source

```bash
git clone https://github.com/talek-solutions/lmn.git
cd lmn
cargo install --path lmn-cli
```

## Verify

```bash
lmn --version
```
