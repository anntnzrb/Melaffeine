{
  perSystem =
    {
      pkgs,
      config,
      rustToolchain,
      ...
    }:
    {
      devShells.default = pkgs.mkShell {
        name = "melaffeine-dev";
        packages = [
          rustToolchain
          pkgs.cargo-nextest
          pkgs.watchexec
          pkgs.just
        ];

        buildInputs = [
          pkgs.apple-sdk_14
        ];

        MACOSX_DEPLOYMENT_TARGET = "14.0";

        shellHook = ''
          ${config.pre-commit.installationScript}
          echo "🦀 Rust $(rustc --version) dev environment loaded!"
        '';
      };
    };
}
