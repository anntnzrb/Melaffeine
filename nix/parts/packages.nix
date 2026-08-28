{ inputs, ... }:
{
  perSystem =
    { pkgs, craneLib, ... }:
    let
      plistFilter =
        path: _type:
        builtins.match ".*Resources/Info\\.plist$" path != null
        || builtins.match ".*/Resources(/.*)?$" path != null;
      src = pkgs.lib.cleanSourceWith {
        src = inputs.self;
        filter = path: type: (plistFilter path type) || (craneLib.filterCargoSources path type);
      };
      darwinFrameworks = [
        pkgs.apple-sdk_14
      ];
      commonArgs = {
        inherit src;
        strictDeps = true;
        buildInputs = darwinFrameworks;
        MACOSX_DEPLOYMENT_TARGET = "14.0";
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      app = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
          pname = "melaffeine";
          cargoExtraArgs = "--package app --bin Melaffeine";
          postInstall = ''
            mkdir -p $out/Applications/Melaffeine.app/Contents/MacOS
            mkdir -p $out/Applications/Melaffeine.app/Contents/Resources
            mv $out/bin/Melaffeine $out/Applications/Melaffeine.app/Contents/MacOS/Melaffeine
            rmdir $out/bin || true
            cp Resources/Info.plist $out/Applications/Melaffeine.app/Contents/Info.plist
          '';
        }
      );
      cli = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
          pname = "melaffeine-cli";
          cargoExtraArgs = "--package melaffeine-cli --bin melaffeine";
        }
      );
    in
    {
      packages = {
        inherit app cli;
        default = app;
        melaffeine = app;
        melaffeine-cli = cli;
      };
    };
}
