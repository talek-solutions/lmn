# Installation

## Homebrew (macOS)

!!! note "Work in progress"
    Homebrew support is coming soon.

## Debian / Ubuntu

```bash
curl -LO https://github.com/talek-solutions/lmn/releases/download/v{{ version }}/lmn_{{ version }}-1_amd64.deb
curl -LO https://github.com/talek-solutions/lmn/releases/download/v{{ version }}/checksums.txt
sha256sum --check --ignore-missing checksums.txt
sudo dpkg -i lmn_{{ version }}-1_amd64.deb
```

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

No Rust toolchain required — pull and run directly.

```bash
docker pull ghcr.io/talek-solutions/lmn:latest
```

**Basic GET test:**

```bash
docker run --rm ghcr.io/talek-solutions/lmn:latest run -H https://httpbin.org/get
```

**With concurrency and request count:**

```bash
docker run --rm ghcr.io/talek-solutions/lmn:latest run -H https://httpbin.org/get -R 1000 -C 50
```

**Run from a config file** — mount the directory containing `lmn.yaml`:

```bash
docker run --rm \
  -v "$PWD:/workspace" -w /workspace \
  ghcr.io/talek-solutions/lmn:latest \
  run -f lmn.yaml
```

**Passing secrets via environment variables:**

```bash
docker run --rm \
  -v "$PWD:/workspace" -w /workspace \
  -e API_TOKEN="my-secret-token" \
  ghcr.io/talek-solutions/lmn:latest \
  run -f lmn.yaml
```

**Testing a locally running service** — use `host.docker.internal` to reach the host machine:

```bash
docker run --rm ghcr.io/talek-solutions/lmn:latest run -H http://host.docker.internal:3000/api
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
