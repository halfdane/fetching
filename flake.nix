{
  description = "librespot development shell";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  outputs = { self, nixpkgs, rust-overlay }:
    let
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.package.version;
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
          pkgs.avahi
          pkgs.dbus
          pkgs.cmake
          pkgs.python3
          pkgs.cargo-edit
          pkgs.cargo-watch
        ];
        shellHook = ''
          export RUST_BACKTRACE=1
        '';
      };
      mkPackage = system: let
        pkgs = if system == "aarch64-linux" then
          import nixpkgs {
            system = "x86_64-linux";
            crossSystem = { config = "aarch64-unknown-linux-gnu"; };
            overlays = [ rust-overlay.overlays.default ];
          }
        else
          import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
      in pkgs.rustPlatform.buildRustPackage {
        pname = "fetching";
        version = version;
        src = ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
        };
        nativeBuildInputs = [ pkgs.pkg-config pkgs.cmake pkgs.nodejs ];
        buildInputs = [
          pkgs.openssl
          pkgs.avahi
          pkgs.nodejs
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
