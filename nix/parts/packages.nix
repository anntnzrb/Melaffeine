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
      # fenix/nixpkgs rustc links nix's libiconv; rewrite the load command to
      # the system dylib so release binaries run on machines without a nix
      # store. Runs in postFixup because fixupPhase stripping invalidates the
      # signature, so re-sign after rewriting.
      fixLibiconv = path: ''
        old=$(otool -L ${path} | awk '/libiconv/{print $1; exit}')
        if [ -n "$old" ]; then
          install_name_tool -change "$old" /usr/lib/libiconv.2.dylib ${path}
        fi
        codesign --force --sign - ${path}
      '';
      commonArgs = {
        inherit src;
        strictDeps = true;
        nativeBuildInputs = [ pkgs.darwin.cctools pkgs.darwin.sigtool ];
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
          postFixup = fixLibiconv "$out/Applications/Melaffeine.app/Contents/MacOS/Melaffeine";
        }
      );
      cli = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
          pname = "melaffeine-cli";
          cargoExtraArgs = "--package melaffeine-cli --bin melaffeine";
          postFixup = fixLibiconv "$out/bin/melaffeine";
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
