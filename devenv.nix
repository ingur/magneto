{ pkgs, lib, ... }:

let
  win = pkgs.pkgsCross.mingwW64;
  winCC = "${win.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
  winAR = "${win.stdenv.cc}/bin/x86_64-w64-mingw32-ar";
in
{
  packages = with pkgs; [
    git
    curl
    wget
    file
    pkg-config
    wrapGAppsHook4
    tcpdump

    openssl
    glib
    glib-networking
    gtk3
    webkitgtk_4_1
    librsvg
    libayatana-appindicator
    xdotool
    # update-desktop-database: the deep-link plugin's register_all() shells
    # out to it (and xdg-mime) when registering the dev binary as the magnet
    # handler on every Linux startup.
    desktop-file-utils

    win.stdenv.cc
  ];

  languages.rust = {
    enable = true;
    channel = "stable";
    targets = [ "x86_64-pc-windows-gnu" ];
  };

  languages.javascript = {
    enable = true;
    package = pkgs.nodejs_22;
    pnpm.enable = true;
  };

  languages.python = {
    enable = true;
    package = pkgs.python3;
    venv = {
      enable = true;
      requirements = ''
        websockets
        requests
      '';
    };
  };

  env.LD_LIBRARY_PATH = lib.makeLibraryPath [
    pkgs.gtk3
    pkgs.webkitgtk_4_1
    pkgs.librsvg
    pkgs.libayatana-appindicator
    pkgs.openssl
  ];

  # win.stdenv.cc exports itself as CC/CXX/AR, which breaks host C builds
  # (librqbit pulls aws-lc-sys).
  env.CC_x86_64_unknown_linux_gnu = "gcc";
  env.CXX_x86_64_unknown_linux_gnu = "g++";
  env.AR_x86_64_unknown_linux_gnu = "ar";

  # libappindicator is dlopen'd by the tray at runtime, so the cc-wrapper
  # rpaths don't cover it: without this the app only starts where
  # LD_LIBRARY_PATH is set (inside this shell), and a handler launch from a
  # browser or file manager panics before single-instance can forward.
  # Per-target, because a global RUSTFLAGS silences the Windows flags below.
  env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS =
    "-C link-arg=-Wl,-rpath,${lib.makeLibraryPath [ pkgs.libayatana-appindicator ]}";

  env.CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = winCC;
  env.CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-L native=${win.windows.pthreads}/lib";
  env.CC_x86_64_pc_windows_gnu = winCC;
  # aws-lc-sys compiles jitterentropy, which needs winpthreads' sched.h.
  env.CFLAGS_x86_64_pc_windows_gnu = "-I${win.windows.pthreads}/include";
  env.AR_x86_64_pc_windows_gnu = winAR;
  env.X86_64_PC_WINDOWS_GNU_OPENSSL_LIB_DIR = "${win.openssl.out}/lib";
  env.X86_64_PC_WINDOWS_GNU_OPENSSL_INCLUDE_DIR = "${win.openssl.dev}/include";

  scripts.dev.exec = "pnpm -C app tauri dev";
  scripts.build.exec = "pnpm -C app tauri build";
  scripts.build-windows.exec = ''
    cargo build --release --target x86_64-pc-windows-gnu "$@"
  '';

  # Run a release build against its own config, data and downloads, so testing
  # cannot touch the installed Magneto. Ports differ too, so both can run.
  scripts.sandbox.exec = ''
    set -euo pipefail
    root="''${MAGNETO_SANDBOX:-/tmp/magneto-sandbox}"

    if pgrep -x magneto-app >/dev/null; then
      echo "magneto-app is already running: quit it first, or single-instance will" >&2
      echo "forward this launch to it and exit (both share the app identifier)." >&2
      exit 1
    fi

    mkdir -p "$root"/{config/magneto,data,cache,downloads}
    if [ ! -f "$root/config/magneto/config.toml" ]; then
      printf '[network]\ncontrol_port = 61581\nlan_port = 61582\n\n[downloads]\ndir = "%s/downloads"\n' \
        "$root" > "$root/config/magneto/config.toml"
    fi

    app=target/release/magneto-app
    if [ ! -x "$app" ] || [ "''${1:-}" = "--build" ]; then
      pnpm -C app tauri build --no-bundle
    fi

    echo "sandbox root: $root"
    XDG_CONFIG_HOME="$root/config" \
    XDG_DATA_HOME="$root/data" \
    XDG_CACHE_HOME="$root/cache" \
      exec "$app"
  '';
}
