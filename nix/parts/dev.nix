{
  perSystem =
    {
      pkgs,
      rustToolchain,
      ...
    }:
    {
      devShells.default = pkgs.mkShell {
        name = "melaffeine-dev";
        packages = [
          rustToolchain
          pkgs.cargo-nextest
          pkgs.cargo-llvm-cov
          pkgs.watchexec
          pkgs.just
        ];

        buildInputs = [
          pkgs.apple-sdk_14
        ];

        MACOSX_DEPLOYMENT_TARGET = "14.0";

        shellHook = ''
          git config core.hooksPath .githooks 2>/dev/null || true
          echo "🦀 Rust $(rustc --version) dev environment loaded!"
        '';
      };
    };
}
