#!/usr/bin/env bash
# Install Ubuntu prerequisites and compile this checkout. Run as your normal user.
set -Eeuo pipefail
trap 'printf "Setup failed at line %s. Resolve the error above and rerun this script.\n" "$LINENO" >&2' ERR
profile=release
jobs=${MOXI_BUILD_JOBS:-2}
install_system=true
while (($#)); do
    case "$1" in
        --debug) profile=dev ;;
        --skip-system) install_system=false ;;
        --jobs) shift; jobs=${1:?--jobs requires a positive integer} ;;
        --help|-h)
            printf 'Usage: %s [--debug] [--skip-system] [--jobs N]\nInstalls Ubuntu dependencies and Rust 1.90.0, then builds with Cargo.lock.\nDefault: release build, two jobs. --skip-system skips apt (dependencies must exist).\n' "$0"
            exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
    shift
done
[[ $jobs =~ ^[1-9][0-9]*$ ]] || { echo 'Jobs must be a positive integer.' >&2; exit 2; }
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_dir"
if $install_system; then
    . /etc/os-release
    [[ ${ID:-} == ubuntu ]] || { echo 'System dependency installation supports Ubuntu. Use --skip-system on a prepared system.' >&2; exit 2; }
    sudo_cmd=()
    if ((EUID != 0)); then
        command -v sudo >/dev/null || { echo 'Install sudo or ask your administrator to install the documented dependencies.' >&2; exit 1; }
        sudo_cmd=(sudo)
    fi
    echo 'Installing compiler, desktop libraries, and native folder chooser…'
    "${sudo_cmd[@]}" apt-get update
    "${sudo_cmd[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates git \
        libx11-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev \
        libxkbcommon-dev libwayland-dev libgl1-mesa-dev libegl1-mesa-dev \
        libfontconfig1-dev libssl-dev xdg-desktop-portal xdg-desktop-portal-gtk
fi
# Prefer rustup's shims when a distro Rust installation is also present.
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
export PATH="$cargo_home/bin:$PATH"
if ! command -v rustup >/dev/null; then
    echo 'Installing rustup from https://sh.rustup.rs…'
    installer=$(mktemp)
    trap 'rm -f -- "${installer:-}"' EXIT
    curl --proto '=https' --tlsv1.2 --fail --show-error --location https://sh.rustup.rs -o "$installer"
    sh "$installer" -y --profile minimal --default-toolchain none --no-modify-path
fi
rustup toolchain install 1.90.0 --profile minimal
# Avoid changing the user's default toolchain or accumulating incremental artifacts.
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
printf 'Building Moxi Box (%s, %s jobs)…\n' "$profile" "$jobs"
cargo +1.90.0 build --locked --profile "$profile" --jobs "$jobs"
target_dir=${CARGO_TARGET_DIR:-"$repo_dir/target"}
[[ $target_dir = /* ]] || target_dir="$repo_dir/$target_dir"
output_profile=$profile
[[ $profile != dev ]] || output_profile=debug
printf '\nBuild complete. Run from your graphical desktop:\n  %q\n' "$target_dir/$output_profile/moxi-box"
echo 'A desktop session with a portal backend is needed for native folder dialogs.'
