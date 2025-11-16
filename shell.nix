{
  pkgs ? import <nixpkgs> { },
}:
let
  # Same library path as default.nix for runtime linking
  libPath =
    with pkgs;
    lib.makeLibraryPath [
      libGL
      libxkbcommon
      wayland
      xorg.libX11
      xorg.libXcursor
      xorg.libXrandr
      xorg.libXi
      gtk3
      libappindicator
    ];
in
pkgs.mkShell {
  # Get dependencies from the main package
  inputsFrom = [ (pkgs.callPackage ./default.nix { }) ];
  # Additional tooling
  buildInputs = with pkgs; [
    rust-analyzer # LSP Server
    rustfmt # Formatter
    clippy # Linter
    cargo
    rustc
  ];

  # Set LD_LIBRARY_PATH so winit can find libraries when running in dev shell
  LD_LIBRARY_PATH = libPath;
}
