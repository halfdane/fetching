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
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
        frontendBuild = pkgs.buildNpmPackage {
          pname = "frontend";
          version = "1.0.0";
          src = ./frontend;
          npmBuildScript = "build";
          installPhase = ''
            cp -r build $out
          '';
          npmDepsHash = "sha256-THISvv1bm9AIJS2EVnO421LsjLYA9YklkDCU1TYPMEI=";
        };
      in pkgs.rustPlatform.buildRustPackage {
        pname = "fetching";
        version = version;
        src = ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
        };
        nativeBuildInputs = [ 
          pkgs.pkg-config 
          pkgs.cmake 
          pkgs.nodejs 
          pkgs.openssl 
        ];
        buildInputs = [
          pkgs.openssl
          pkgs.avahi
          pkgs.nodejs
        ];
        postPatch = ''
          mkdir -p frontend/build
          cp -r ${frontendBuild}/* frontend/build/
        '';

        buildPhase = ''
          cargo build --release
        '';
        installPhase = ''
          mkdir -p $out/bin $out/pwa
          cp target/release/fetching $out/bin/
          cp -r frontend/build/* $out/pwa/
        '';
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
