{ ... }:
{
  # adjust this path, or import the flake's homeManagerModules.default.
  imports = [ ../nix/home-manager.nix ];

  programs.waybar-pomodoro = {
    enable = true;
    bars = [ "laptop" "monitor" ];
    settings = {
      focus_minutes = 25;
      short_break_minutes = 5;
      long_break_minutes = 30;
      focuses_before_long_break = 4;
      notifications = true;
    };
  };

  programs.waybar.settings = {
    laptop.modules-center = [ "clock" "custom/pomodoro" "custom/dnd" ];
    monitor.modules-center = [ "clock" "custom/pomodoro" "custom/dnd" ];
  };
}
