{
  lib,
  rustPlatform,
  git,
  makeWrapper,
}:
let
  cargo = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package;
in
rustPlatform.buildRustPackage {
  pname = cargo.name;
  inherit (cargo) version;

  src = lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    git
    makeWrapper
  ];

  # Source management invokes Git at runtime, including from Nix profiles.
  postInstall = ''
    wrapProgram "$out/bin/dalo" --prefix PATH : ${lib.makeBinPath [ git ]}
    echo nix > "$out/bin/.dalo-install-channel"
  '';

  meta = {
    inherit (cargo) description;
    homepage = "https://dalo.sh";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "dalo";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
}
