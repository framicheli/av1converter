{
  description = "A terminal-based interactive tool to batch convert video files to the AV1 codec using FFmpeg";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          # ffmpeg and ffprobe are run from PATH at runtime and are not added to
          # the package. The Dolby Vision and VMAF features the tool uses depend
          # on how that ffmpeg was built (see README prerequisites).

          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.repository;
            license = licenses.mit;
            mainProgram = cargoToml.package.name;
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rust-analyzer
            pkgs.rustfmt
            pkgs.clippy
            pkgs.ffmpeg-full
          ];
        };
      }
    );
}
