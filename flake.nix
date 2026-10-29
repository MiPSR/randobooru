{
  description = "randobooru - Rust Discord bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }: let
    systems = [ "x86_64-linux" ];
    forAllSystems = nixpkgs.lib.genAttrs systems;
  in {
    packages = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "randobooru";
        version = "26.1.1-alpha";
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./build.rs
            ./src
            ./locales
            ./LICENSE
          ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.openssl ];
        meta = {
          description = "Discord bot that returns random booru images from configurable tag presets";
          mainProgram = "randobooru";
          platforms = [ "x86_64-linux" ];
        };
      };
    });

    apps = forAllSystems (system: {
      default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/randobooru";
        meta.description = "Run randobooru from a provisioned runtime state directory";
      };
    });

    devShells = forAllSystems (system: let pkgs = nixpkgs.legacyPackages.${system}; in {
      default = pkgs.mkShell {
        packages = with pkgs; [ cargo rustc rustfmt clippy cargo-audit pkg-config openssl ];
        RUST_BACKTRACE = "1";
      };
    });

    checks = forAllSystems (system: let
      pkgs = nixpkgs.legacyPackages.${system};
      testSystem = nixpkgs.lib.nixosSystem {
        modules = [ self.nixosModules.default {
          nixpkgs.hostPlatform = system;
          system.stateVersion = "26.05";
          services.randobooru = {
            enable = true;
            secretsFile = "/var/lib/randobooru/runtime-only.toml";
            dbPath = "audit.sqlite3";
          };
        } ];
      };
      service = testSystem.config.systemd.services.randobooru;
      source = self.packages.${system}.default.src;
    in {
      module-contract =
        assert service.environment.RANDOBOORU_SECRETS_FILE == "/var/lib/randobooru/runtime-only.toml";
        assert service.environment.RANDOBOORU_DB_PATH == "audit.sqlite3";
        assert service.serviceConfig.Restart == "on-failure";
        assert !(builtins.hasAttr "secretsText" testSystem.options.services.randobooru);
        assert !(pkgs.lib.hasInfix "printf" service.preStart);
        pkgs.runCommand "randobooru-module-contract" {} "touch $out";
      source-contract = pkgs.runCommand "randobooru-source-contract" { src = source; } ''
        test -f "$src/Cargo.toml"
        test -d "$src/src"
        test ! -e "$src/secrets.toml"
        test ! -e "$src/randobooru.sqlite3"
        test ! -e "$src/target"
        test ! -e "$src/.git"
        touch "$out"
      '';
    });

    nixosModules.default = { config, lib, pkgs, ... }: let
      cfg = config.services.randobooru;
    in {
      options.services.randobooru = {
        enable = lib.mkEnableOption "randobooru Discord bot";
        package = lib.mkOption {
          type = lib.types.package;
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };
        dataDir = lib.mkOption {
          type = lib.types.str;
          default = "/var/lib/randobooru";
        };
        secretsFile = lib.mkOption {
          type = lib.types.str;
          default = "${cfg.dataDir}/secrets.toml";
          description = "Absolute runtime path to a manually provisioned secrets file. Never put secret contents in Nix configuration.";
        };
        dbPath = lib.mkOption {
          type = lib.types.str;
          default = "randobooru.sqlite3";
        };
      };

      config = lib.mkIf cfg.enable {
        assertions = [
          { assertion = lib.hasPrefix "/" cfg.dataDir; message = "randobooru.dataDir must be absolute"; }
          { assertion = lib.hasPrefix "/" cfg.secretsFile && !(lib.hasPrefix builtins.storeDir cfg.secretsFile); message = "randobooru.secretsFile must be a runtime path outside the Nix store"; }
        ];
        users.users.randobooru = {
          isSystemUser = true;
          group = "randobooru";
          home = cfg.dataDir;
          createHome = true;
        };
        users.groups.randobooru = {};
        systemd.tmpfiles.rules = [ "d ${cfg.dataDir} 0700 randobooru randobooru -" ];

        systemd.services.randobooru = {
          description = "randobooru Discord bot";
          after = [ "network-online.target" ];
          wants = [ "network-online.target" ];
          wantedBy = [ "multi-user.target" ];
          environment = {
            RANDOBOORU_SECRETS_FILE = cfg.secretsFile;
            RANDOBOORU_DB_PATH = cfg.dbPath;
            SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          };
          serviceConfig = {
            Type = "simple";
            User = "randobooru";
            Group = "randobooru";
            WorkingDirectory = cfg.dataDir;
            ExecStart = "${cfg.package}/bin/randobooru";
            Restart = "on-failure";
            RestartSec = "5s";
            UMask = "0077";
            NoNewPrivileges = true;
            PrivateTmp = true;
            ProtectSystem = "strict";
            ProtectHome = true;
            ReadWritePaths = [ cfg.dataDir ];
            TimeoutStopSec = "60s";
          };
          preStart = ''
            if ! test -r ${lib.escapeShellArg cfg.secretsFile}; then
              echo ${lib.escapeShellArg ("randobooru: runtime secrets file is missing or unreadable: " + cfg.secretsFile)} >&2
              exit 1
            fi
          '';
        };
      };
    };
  };
}
