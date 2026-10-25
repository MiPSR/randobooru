{
  description = "randobooru - Rust Discord bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }: let
    systems = [ "x86_64-linux" "aarch64-linux" ];
    forAllSystems = nixpkgs.lib.genAttrs systems;
  in {
    packages = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "randobooru";
        version = "26.1.1-alpha";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = with pkgs.lib; {
          description = "Discord bot that returns random booru images from configurable tag presets";
          mainProgram = "randobooru";
          platforms = platforms.linux;
        };
      };
    });

    apps = forAllSystems (system: {
      default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/randobooru";
      };
    });

    nixosModules.default = { config, lib, pkgs, ... }: let
      cfg = config.services.randobooru;
    in {
      options.services.randobooru = {
        enable = lib.mkEnableOption "randobooru Discord bot";
        package = lib.mkOption {
          type = lib.types.package;
          default = self.packages.${pkgs.system}.default;
        };
        dataDir = lib.mkOption {
          type = lib.types.path;
          default = "/var/lib/randobooru";
        };
        secretsText = lib.mkOption {
          type = lib.types.str;
          default = "";
          description = "Contents of secrets.toml";
        };
        dbPath = lib.mkOption {
          type = lib.types.str;
          default = "randobooru.sqlite3";
        };
      };

      config = lib.mkIf cfg.enable {
        users.users.randobooru = {
          isSystemUser = true;
          group = "randobooru";
          home = cfg.dataDir;
          createHome = true;
        };
        users.groups.randobooru = {};

        systemd.services.randobooru = {
          description = "randobooru Discord bot";
          after = [ "network-online.target" ];
          wants = [ "network-online.target" ];
          wantedBy = [ "multi-user.target" ];
          serviceConfig = {
            Type = "simple";
            User = "randobooru";
            Group = "randobooru";
            WorkingDirectory = cfg.dataDir;
            ExecStart = "${cfg.package}/bin/randobooru";
            Restart = "on-failure";
            RestartSec = "5s";
          };
          preStart = ''
            mkdir -p ${cfg.dataDir}
            chown randobooru:randobooru ${cfg.dataDir}
            if [ -n ${lib.escapeShellArg cfg.secretsText} ]; then
              printf '%s\n' ${lib.escapeShellArg cfg.secretsText} > ${cfg.dataDir}/secrets.toml
              chown randobooru:randobooru ${cfg.dataDir}/secrets.toml
              chmod 600 ${cfg.dataDir}/secrets.toml
            fi
          '';
        };
      };
    };
  };
}