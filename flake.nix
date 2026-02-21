{
  description = "librespot development shell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "aarch64-linux" "x86_64-linux" ];
      mkDevShell = system: let
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      in pkgs.mkShell {
        buildInputs = [
          (pkgs.rust-bin.stable.latest.default)
          pkgs.rust-analyzer
          pkgs.rustup
          pkgs.pkg-config
          pkgs.openssl
          pkgs.alsa-lib
          pkgs.avahi
          pkgs.libpulseaudio
          pkgs.dbus
          pkgs.cmake
          pkgs.python3
          pkgs.vorbis-tools # for ogginfo, vorbiscomment, etc.
          pkgs.cargo-edit
        ];
        shellHook = ''
          export RUST_BACKTRACE=1
        '';
      };
      mkPackage = system: let
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      in pkgs.rustPlatform.buildRustPackage {
        pname = "fetching";
        version = "0.1.0";
        src = ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
        };
        nativeBuildInputs = [ pkgs.pkg-config pkgs.cmake ];
        buildInputs = [
          pkgs.openssl
          pkgs.alsa-lib
          pkgs.avahi
          pkgs.libpulseaudio
          pkgs.dbus
          pkgs.vorbis-tools
        ];
      };
    in {
      devShells = builtins.listToAttrs (map (system: {
        name = system;
        value = { default = mkDevShell system; };
      }) systems);
      packages = builtins.listToAttrs (map (system: {
        name = system;
        value = { default = mkPackage system; };
      }) systems);
    };
}
