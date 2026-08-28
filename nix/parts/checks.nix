{ inputs, ... }:
{
  perSystem =
    {
      self',
      pkgs,
      craneLib,
      ...
    }:
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
    in
    {
      checks = {
        app = self'.packages.default;
        clippy = craneLib.cargoClippy (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
          }
        );
        nextest = craneLib.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestExtraArgs = "--workspace --no-tests=pass";
          }
        );
      };
    };
}
