{ config, lib, pkgs, ... }:
let
  cfg = config.programs.waybar-pomodoro;
  moduleSettings = lib.mapAttrsRecursive (_: lib.mkDefault)
    ((builtins.fromJSON (builtins.readFile ../examples/waybar.jsonc))."custom/pomodoro" // {
      exec = "${cfg.package}/bin/waybar-pomodoro watch --text-only";
      on-click = "${cfg.package}/bin/waybar-pomodoro start";
      on-click-right = "${cfg.package}/bin/waybar-pomodoro pause";
      on-click-middle = "${cfg.package}/bin/waybar-pomodoro reset";
    });
in {
  options.programs.waybar-pomodoro = {
    enable = lib.mkEnableOption "pomodoro timer for waybar";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../package.nix {};
      description = "the waybar-pomodoro package";
    };
    settings = lib.mkOption {
      type = (pkgs.formats.toml {}).type;
      default = {};
      description = "timer settings; omitted values use the built-in defaults";
    };
    bars = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      description = "waybar settings names to add the custom module to; set modules-center separately";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
    xdg.configFile."waybar-pomodoro/config.toml".source =
      (pkgs.formats.toml {}).generate "waybar-pomodoro-config.toml" cfg.settings;
    programs.waybar.settings = lib.genAttrs cfg.bars (_: {
      "custom/pomodoro" = moduleSettings;
    });
  };
}
