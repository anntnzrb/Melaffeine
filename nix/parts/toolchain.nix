{ inputs, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      rustToolchain = inputs.fenix.packages.${system}.fromToolchainFile {
        file = ../../rust-toolchain.toml;
        sha256 = "sha256-P30Tm3O7vQAE725YtDCDHGjNrSsfZO4us11UwJGZSJo=";
      };
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
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
        nativeBuildInputs = [
          pkgs.darwin.cctools
          pkgs.darwin.sigtool
        ];
        buildInputs = darwinFrameworks;
        MACOSX_DEPLOYMENT_TARGET = "14.0";
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in
    {
      _module.args = {
        inherit
          rustToolchain
          craneLib
          commonArgs
          cargoArtifacts
          ;
      };
    };
}
