{
  pkgs ? import <nixpkgs> { },
}:
let
  manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
  cross = pkgs.pkgsCross.musl64;
in
cross.rustPlatform.buildRustPackage rec {
  pname = manifest.name;
  version = manifest.version;

  cargoLock.lockFile = ./Cargo.lock;

  RUSTFLAGS = "-C target-feature=+crt-static";

  src = cross.lib.cleanSource ./.;
  buildType = "release";

  nativeBuildInputs = [ cross.musl ];
  buildInputs = [ ];

  # Provide font path at build time
  preBuild = ''
    export FONT_PATH="${pkgs.nerd-fonts.roboto-mono}/share/fonts/truetype/NerdFonts/RobotoMono/RobotoMonoNerdFont-Regular.ttf"
  '';
}
