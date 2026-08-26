# cosmic-player
WIP COSMIC media player

# How To Install

**COSMIC Player** is a Rust-based, GStreamer-powered media player built for the [COSMIC desktop environment](https://github.com/pop-os/cosmic-epoch), developed by [System76](https://system76.com). It uses Vulkan for rendering and VA-API for hardware-accelerated video decoding.

> ⚠️ This project is a work in progress (WIP). The interface and commands may change over time.

- **Source code:** <https://github.com/pop-os/cosmic-player>
- **License:** GPL-3.0-or-later

---

## Table of Contents

- [Installation](#installation)
  - [Arch Linux and derivatives](#arch-linux-and-derivatives)
  - [Fedora / RHEL-based distributions](#fedora--rhel-based-distributions)
  - [Debian / Ubuntu-based distributions](#debian--ubuntu-based-distributions)
  - [Installing via Flatpak](#installing-via-flatpak)
- [Building from source](#building-from-source)
- [Usage](#usage)
- [Contributing](#contributing)
- [License](#license)

---

## Installation

### Arch Linux and derivatives

`cosmic-player` is available in Arch Linux's official **extra** repository as part of the `cosmic` group, and can be installed directly with `pacman`:

```bash
sudo pacman -Syu cosmic-player
```

The package name is the same on derivatives (Manjaro, EndeavourOS, etc.). If it isn't available yet on your system, you can install the `cosmic-player-git` package from the AUR using an AUR helper (`yay`, `paru`, etc.):

```bash
yay -S cosmic-player-git
```

### Fedora / RHEL-based distributions

`cosmic-player` is available in the official repositories of current Fedora releases:

```bash
sudo dnf install cosmic-player
```

If the package isn't found in the official repositories (older Fedora releases, or systems where COSMIC hasn't been officially packaged yet), you can enable the community-maintained COPR repository instead:

```bash
sudo dnf copr enable ryanabx/cosmic-epoch
sudo dnf install cosmic-player
```

On enterprise derivatives such as RHEL, AlmaLinux, or Rocky Linux, an official package may not be available. In that case, follow the [Building from source](#building-from-source) section instead.

> ℹ️ COPR repositories are not officially supported by the Fedora project. If you run into issues, please report them to the COPR project owner rather than Fedora Bugzilla.

### Debian / Ubuntu-based distributions

`cosmic-player` is available in Pop!\_OS's official repositories and can be installed with `apt`:

```bash
sudo apt update
sudo apt install cosmic-player
```

On distributions that don't officially package the COSMIC desktop (such as plain Debian or Ubuntu), the package may not be available in your default repositories. In that case:

1. Add Pop!\_OS's COSMIC package repository to your system, **or**
2. Follow the [Building from source](#building-from-source) section instead.

### Installing via Flatpak

COSMIC applications are typically surfaced through the built-in **COSMIC Store**, which aggregates Flathub and other configured Flatpak sources. Whether `cosmic-player` is published as a standalone package on Flathub can vary, so it's best to check availability before installing:

```bash
# If you haven't already added the Flathub remote:
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo

# Search Flathub for cosmic-player:
flatpak search cosmic-player
```

If the search returns an application ID, you can install it with:

```bash
flatpak install flathub <application-id-found>
```

Alternatively, you can check Pop!\_OS's community Flatpak repository for COSMIC components:

```bash
flatpak remote-add --if-not-exists --user cosmic https://apt.pop-os.org/cosmic/cosmic.flatpakrepo
flatpak search cosmic-player
```

> ⚠️ `cosmic-player` may not be listed as a standalone Flatpak package on every source. If it isn't available, use the native package for your distribution (see [Installation](#installation)) or [build it from source](#building-from-source).

---

## Building from source

The steps below walk you through cloning the Git repository and manually building and installing `cosmic-player`.

### 1. Install the required dependencies

Building the project requires the Rust toolchain (`cargo`), the `just` command runner, and GStreamer/Wayland development libraries.

**Arch Linux:**

```bash
sudo pacman -S --needed base-devel git rust just clang lld \
    gstreamer gst-plugins-base gst-plugins-good libxkbcommon
```

**Fedora / RHEL-based:**

```bash
sudo dnf install git rust cargo just clang lld gcc gcc-c++ \
    gstreamer1-devel gstreamer1-plugins-base-devel \
    libxkbcommon-devel wayland-devel
```

**Debian / Ubuntu-based:**

```bash
sudo apt update
sudo apt install git cargo rustc just clang lld pkg-config \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-good1.0-dev libxkbcommon-dev libwayland-dev
```

> ℹ️ If `just` isn't available in your package repositories, you can install it with `cargo install just`. It's also recommended to keep your Rust toolchain up to date using [rustup](https://rustup.rs).

### 2. Clone the repository

```bash
git clone https://github.com/pop-os/cosmic-player.git
cd cosmic-player
```

### 3. Build

Using `just` (recommended):

```bash
just build-release
```

Using `cargo` directly:

```bash
cargo build --release
```

### 4. Install

To install to your system with `just`:

```bash
sudo just install
```

This places the binary, the `.desktop` entry, and icons into the appropriate system directories (defaults to `/usr/local`). To use a different prefix:

```bash
sudo just prefix=/usr install
```

### 5. Run without installing (optional)

To test the built binary without installing it system-wide:

```bash
cargo run --release
```

or:

```bash
just run
```

---

## Usage

After installation, you can launch `cosmic-player` from your application launcher, or run it directly from a terminal:

```bash
cosmic-player /path/to/file.mp4
```

---

## Contributing

Please use [GitHub Issues](https://github.com/pop-os/cosmic-player/issues) to report bugs or request features. For code contributions, review the existing pull requests in the repository and open a new pull request when ready.

## License

This project is distributed under the [GPL-3.0-or-later](https://github.com/pop-os/cosmic-player/blob/master/LICENSE) license.
