{ config, lib, pkgs, ... }:

let
  cfg = config.programs.cwt;
  tomlFormat = pkgs.formats.toml { };
in
{
  options.programs.cwt = {
    enable = lib.mkEnableOption "cwt — Claude Worktree Manager";

    package = lib.mkPackageOption pkgs "cwt" { };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      description = "Configuration written to {file}`~/.config/cwt/config.toml`.";
      example = lib.literalExpression ''
        {
          session = {
            default_permission = "elevated";
            permissions.elevated.settings_override.sandbox = {
              enabled = true;
              autoAllowBashIfSandboxed = true;
            };
          };
        }
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."cwt/config.toml" = lib.mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "cwt-config" cfg.settings;
    };
  };
}
