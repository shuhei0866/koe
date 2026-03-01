#!/bin/bash
# Build script for koe - sets up environment for local dev packages
# If you've installed system packages (sudo apt install libasound2-dev libclang-dev libxkbcommon-dev
# libx11-dev libxi-dev libxext-dev libxtst-dev libxfixes-dev cmake), you can just run: cargo build

LOCAL_PKGS="$HOME/local-pkgs"

if [ -d "$LOCAL_PKGS" ]; then
    export PATH="$LOCAL_PKGS/usr/bin:$PATH"
    export PKG_CONFIG_PATH="$LOCAL_PKGS/usr/lib/x86_64-linux-gnu/pkgconfig:$LOCAL_PKGS/usr/share/pkgconfig:${PKG_CONFIG_PATH:-}"
    export C_INCLUDE_PATH="$LOCAL_PKGS/usr/include:/usr/lib/gcc/x86_64-linux-gnu/13/include:${C_INCLUDE_PATH:-}"
    export CPATH="$LOCAL_PKGS/usr/include:/usr/lib/gcc/x86_64-linux-gnu/13/include:${CPATH:-}"
    export LIBRARY_PATH="$LOCAL_PKGS/usr/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:${LIBRARY_PATH:-}"
    export LD_LIBRARY_PATH="$LOCAL_PKGS/usr/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
    export CMAKE_PREFIX_PATH="$LOCAL_PKGS/usr"
    # Ensure the linker can find unversioned .so files from dev packages
    export RUSTFLAGS="-L $LOCAL_PKGS/usr/lib/x86_64-linux-gnu ${RUSTFLAGS:-}"
fi

export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")" || exit 1
cargo build "$@"
