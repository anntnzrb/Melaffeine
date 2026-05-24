{ pkgs, src }:

let
  melaffeine = pkgs.stdenv.mkDerivation {
    pname = "melaffeine";
    version = "0.1.0";

    inherit src;

    nativeBuildInputs = [
      pkgs.just
    ];

    doCheck = true;

    buildPhase = ''
      runHook preBuild
      XATTR=true CODESIGN=true just build
      runHook postBuild
    '';

    checkPhase = ''
      runHook preCheck
      just test
      runHook postCheck
    '';

    installPhase = ''
      runHook preInstall
      mkdir -p "$out/Applications"
      cp -R Melaffeine.app "$out/Applications/"
      runHook postInstall
    '';

    meta = {
      description = "Tiny native macOS menu-bar utility for keeping the Mac awake";
      mainProgram = "Melaffeine";
      platforms = pkgs.lib.platforms.darwin;
    };
  };
in
{
  inherit melaffeine;
  default = melaffeine;
}
