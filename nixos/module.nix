self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.fetching;
  fetchingPkg = self.packages.${pkgs.system}.default;
in
{
  options.services.fetching = {
    enable = lib.mkEnableOption "fetching Spotify music downloader";

    package = lib.mkOption {
      type = lib.types.package;
      default = fetchingPkg;
      description = "The fetching package to use.";
    };

    credentialsFile = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/fetching/credentials.json";
      description = "Path to the Spotify credentials JSON file.";
    };

    outputDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/fetching/music";
      description = "Directory where downloaded music files are stored.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "HTTP port for the web UI.";
    };

    concurrency = lib.mkOption {
      type = lib.types.int;
      default = 1;
      description = "Maximum number of parallel downloads.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "fetching";
      description = "User under which fetching runs.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "fetching";
      description = "Group under which fetching runs.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the firewall for the web UI port.";
    };

    nginx = {
      enable = lib.mkEnableOption "nginx reverse proxy for fetching";

      hostName = lib.mkOption {
        type = lib.types.str;
        default = "fetching.localhost";
        description = "The hostname for the nginx virtual host.";
      };

      forceSSL = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Redirect HTTP to HTTPS on the virtual host.";
      };

      acmeHost = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = ''
          Use the ACME certificate for this hostname.
          Implies forceSSL is likely desired.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = lib.mkIf (cfg.user == "fetching") {
      isSystemUser = true;
      group = cfg.group;
      home = "/var/lib/fetching";
      createHome = true;
    };

    users.groups.${cfg.group} = lib.mkIf (cfg.group == "fetching") { };

    systemd.tmpfiles.rules = [
      "d ${cfg.outputDir} 0755 ${cfg.user} ${cfg.group} -"
      "d ${builtins.dirOf cfg.credentialsFile} 0700 ${cfg.user} ${cfg.group} -"
    ];

    systemd.services.fetching = {
      description = "fetching Spotify music downloader";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        Group = cfg.group;
        ExecStart = lib.concatStringsSep " " [
          "${cfg.package}/bin/fetching"
          "serve"
          "--credentials" cfg.credentialsFile
          "--output" cfg.outputDir
          "--port" (toString cfg.port)
          "--concurrency" (toString cfg.concurrency)
        ];
        Restart = "on-failure";
        RestartSec = "10s";
        StateDirectory = "fetching";
        CacheDirectory = "fetching";

        # Hardening
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ReadWritePaths = [
          cfg.outputDir
          (builtins.dirOf cfg.credentialsFile)
          "/var/cache/fetching"
        ];
        ProtectHome = true;
        PrivateTmp = true;
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];

    services.nginx = lib.mkIf cfg.nginx.enable {
      enable = true;
      virtualHosts.${cfg.nginx.hostName} = {
        forceSSL = cfg.nginx.forceSSL;
        useACMEHost = cfg.nginx.acmeHost;
        locations."/" = {
          proxyPass = "http://127.0.0.1:${toString cfg.port}";
          proxyWebsockets = true;
        };
      };
    };
  };
}
