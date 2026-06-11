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
    # The tray's libappindicator is dlopen'd at runtime, not linked, so the
    # cc-wrapper rpaths don't cover it. Without this rpath the app only starts
    # where LD_LIBRARY_PATH is set (inside this shell); a handler launch from
    # a browser or file manager panics before single-instance can forward.
    rustflags = "-C link-arg=-Wl,-rpath,${lib.makeLibraryPath [ pkgs.libayatana-appindicator ]}";
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

  env.CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = winCC;
  env.CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-L native=${win.windows.pthreads}/lib";
  env.CC_x86_64_pc_windows_gnu = winCC;
  env.AR_x86_64_pc_windows_gnu = winAR;
  env.X86_64_PC_WINDOWS_GNU_OPENSSL_LIB_DIR = "${win.openssl.out}/lib";
  env.X86_64_PC_WINDOWS_GNU_OPENSSL_INCLUDE_DIR = "${win.openssl.dev}/include";

  scripts.dev.exec = "pnpm -C app tauri dev";
  scripts.build.exec = "pnpm -C app tauri build";
  scripts.build-windows.exec = ''
    cargo build --release --target x86_64-pc-windows-gnu "$@"
  '';
}
