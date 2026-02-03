{
  description = "librespot development shell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  outputs = { self, nixpkgs, rust-overlay }:
    let
      pkgs = import nixpkgs { system = "aarch64-linux"; overlays = [ rust-overlay.overlays.default ]; };
    in {
      devShells.aarch64-linux.default = pkgs.mkShell {
        buildInputs = [
          (pkgs.rust-bin.stable.latest.default)
          pkgs.pkg-config
          pkgs.openssl
          pkgs.alsa-lib
          pkgs.avahi
          pkgs.libpulseaudio
          pkgs.dbus
          pkgs.cmake
          pkgs.python3
          pkgs.vorbis-tools # for ogginfo, vorbiscomment, etc.
        ];
        shellHook = ''
          export RUST_BACKTRACE=1
        '';
      };
    };
}
