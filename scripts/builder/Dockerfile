# The machine the CI builds run on, and a developer's toolchain in a box:
# Ubuntu 24.04 with what ESP-IDF's build needs, the pinned stable toolchain
# for the host workspace with the bare-metal target, the pinned nightly
# with rust-src for the firmware crates, ldproxy and esptool. A job installs
# no system package when it runs, so it never waits for a package mirror.
#
# ESP-IDF itself and the chip's GCC are not in the image: embuild fetches
# them on the first build into the firmware crate's .embuild/, which lives
# in the mounted checkout and so survives between runs.
#
# On a developer's machine, from the repository's root:
#
#   docker build -t companion-builder -f scripts/builder/Dockerfile .
#   docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD:/work" -w /work companion-builder scripts/check.sh
#
# The context is the repository's root, of which the build reads only the
# toolchain files (see Dockerfile.dockerignore).
FROM ubuntu:24.04

LABEL org.opencontainers.image.source=https://github.com/foonerd/volumio-esp-companion
LABEL org.opencontainers.image.description="The machine Volumio ESP Companion's CI builds run on"

# The apt list is the one esp-rs/esp-idf-template's devcontainer installs,
# plus shellcheck for check.sh and libsdl2-dev for the simulator's window.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates curl file git xz-utils zip unzip shellcheck libsdl2-dev \
    gcc libc6-dev make binutils pkg-config clang libudev-dev \
    wget flex bison gperf python3 python3-pip python3-venv \
    cmake ninja-build ccache libffi-dev libssl-dev dfu-util libusb-1.0-0 \
    && rm -rf /var/lib/apt/lists/*

# The checkout a build works in belongs to whoever mounted it, not to the
# user inside; git answers for it all the same.
RUN git config --system --add safe.directory '*'

# esptool, for scripts/image.sh. System-wide: this is a build box.
ARG ESPTOOL_VERSION=5.6.0
RUN pip install --no-cache-dir --break-system-packages "esptool==$ESPTOOL_VERSION"

# The two Rust toolchains the repository names: stable for the host
# workspace (plus the bare-metal RISC-V target check.sh builds for), the
# pinned nightly with rust-src for the firmware crates. Cargo's home is
# writable by any user, so the image also serves a developer's own uid.
ARG RUSTUP_VERSION=1.29.1
ARG RUSTUP_SHA256=dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
COPY rust-toolchain.toml /tmp/host/rust-toolchain.toml
COPY firmware/waveshare-esp32-c5-touch-lcd-28/rust-toolchain.toml /tmp/firmware/rust-toolchain.toml
RUN curl -fsSL -o /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain none \
    && (cd /tmp/host && rustup toolchain install && rustup target add riscv32imac-unknown-none-elf) \
    && (cd /tmp/firmware && rustup toolchain install) \
    && rustup default "$(cd /tmp/host && rustup show active-toolchain | cut -d' ' -f1)" \
    && rm -rf /tmp/rustup-init /tmp/host /tmp/firmware

# ldproxy, the linker shim the firmware's .cargo/config.toml names. embuild's
# GitHub release stops at 0.3.2; 0.3.5 is on crates.io, so it is built.
ARG LDPROXY_VERSION=0.3.5
RUN cargo install --locked --version "$LDPROXY_VERSION" ldproxy \
    && rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git" \
    && chmod -R a+rwX "$CARGO_HOME"
