{ pkgs, lib, ... }:

let
  # Runtime libraries GPUI loads dynamically (windowing, GPU, fonts).
  gpuiLibs = with pkgs; [
    wayland
    libxkbcommon
    libxcb
    xorg.libX11
    vulkan-loader
    fontconfig
    freetype
  ];
in
{
  # https://devenv.sh/languages/rust/
  languages.rust = {
    enable = true;
    # Latest stable toolchain via rust-overlay
    channel = "stable";
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-src" ];
  };

  packages = with pkgs; [
    git
    pkg-config
    clang
    openssl
    zstd
    alsa-lib
    vulkan-headers
  ] ++ gpuiLibs;

  env = {
    RUST_BACKTRACE = "1";
    LD_LIBRARY_PATH = lib.makeLibraryPath gpuiLibs;
  };

  enterShell = ''
    rustc --version
    cargo --version
  '';

  enterTest = ''
    rustc --version
    cargo --version
    cargo check
  '';
}
