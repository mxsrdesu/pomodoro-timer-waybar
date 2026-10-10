{
  description = "pomodoro timer for waybar";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
    in {
      packages = forEachSystem (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.callPackage ./package.nix {};
          waybar-pomodoro = self.packages.${system}.default;
        });

      apps = forEachSystem (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/waybar-pomodoro";
        };
      });

      devShells = forEachSystem (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ cargo rustc rustfmt clippy rust-analyzer libnotify ];
          };
        });

      checks = forEachSystem (system: {
        default = self.packages.${system}.default;
      });

      homeManagerModules.default = import ./nix/home-manager.nix;
    };
}
