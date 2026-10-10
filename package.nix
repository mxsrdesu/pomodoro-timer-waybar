{ lib, rustPlatform, makeWrapper, libnotify }:

rustPlatform.buildRustPackage {
  pname = "waybar-pomodoro";
  version = "0.1.2";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./tests ./examples ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ makeWrapper ];
  strictDeps = true;
  doCheck = true;

  postInstall = ''
    wrapProgram "$out/bin/waybar-pomodoro" \
      --prefix PATH : ${lib.makeBinPath [ libnotify ]}
    install -Dm644 examples/config.toml "$out/share/waybar-pomodoro/config.toml"
    install -Dm644 examples/waybar.jsonc "$out/share/waybar-pomodoro/waybar.jsonc"
  '';

  meta = {
    description = "pomodoro timer for waybar";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "waybar-pomodoro";
  };
}
