{
  description = "fetching — Spotify music fetcher with CLI batch mode and web UI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    fetching-cli.url = "github:halfdane/fetching-cli";
  };

  outputs = { self, nixpkgs, flake-utils, fetching-cli }:
    flake-utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        fetchingCliPkg = fetching-cli.packages.${system}.default;
      in
      {
        packages.default =
          let fetchingVersion = "0.1.7"; in
          pkgs.buildGoModule {
            pname = "fetching";
            version = fetchingVersion;
            src = ./.;
            vendorHash = "sha256-j2cQfEzSaa10+boHb79EhF/1fsp2rwCGbTg/+Q7r2c4=";
            ldflags = [ "-X main.version=v${fetchingVersion}" ];
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postInstall = ''
              wrapProgram $out/bin/fetching \
                --prefix PATH : ${fetchingCliPkg}/bin:${pkgs.ffmpeg}/bin
            '';
            meta = {
              description = "Spotify music fetcher with CLI batch mode and web UI";
              mainProgram = "fetching";
            };
          };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            go
            gopls
            gotools
            go-tools # staticcheck
            fetchingCliPkg
            ffmpeg
          ];
        };
      }
    ) // {
      nixosModules.default = import ./nixos/module.nix self;
    };
}
