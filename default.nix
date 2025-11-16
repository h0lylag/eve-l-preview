{
  pkgs ? import <nixpkgs> { },
}:
let
  manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
  # Library path for runtime dynamic loading (winit/wgpu need this)
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
      xdotool
    ];
in
pkgs.rustPlatform.buildRustPackage rec {
  pname = manifest.name;
  version = manifest.version;

  cargoLock.lockFile = ./Cargo.lock;

  src = pkgs.lib.cleanSource ./.;

  # Skip tests in build
  doCheck = false;

  nativeBuildInputs = with pkgs; [
    makeWrapper
    pkg-config
  ];

  buildInputs = with pkgs; [
    libGL
    libxkbcommon
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXrandr
    xorg.libXi
    gtk3
    libappindicator
    xdotool
  ];

  # Set LD_LIBRARY_PATH so winit can find libraries at runtime
  postInstall = ''
    wrapProgram $out/bin/eve-l-preview \
      --prefix LD_LIBRARY_PATH : "${libPath}"
  '';

  # Provide font path at build time
  preBuild = ''
    export FONT_PATH="${pkgs.nerd-fonts.roboto-mono}/share/fonts/truetype/NerdFonts/RobotoMono/RobotoMonoNerdFont-Regular.ttf"
  '';
}
