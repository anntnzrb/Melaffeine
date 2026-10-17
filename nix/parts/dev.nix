{
  perSystem =
    {
      pkgs,
      rustToolchain,
      ...
    }:
    let
      cargoDeny = pkgs.rustPlatform.buildRustPackage {
        pname = "cargo-deny";
        version = "0.20.2";

        src = pkgs.fetchCrate {
          pname = "cargo-deny";
          version = "0.20.2";
          hash = "sha256-MYE/YO5v46VxatY1/Asw+hXu9GKMdDKhnXvIGuYTCQc=";
        };

        cargoHash = "sha256-Zb6vQCnhhhL9Ducn9eh5P8Gfopl0lQPTXWW8Q0Y5xBQ=";
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.zstd ];
        env.ZSTD_SYS_USE_PKG_CONFIG = true;

        # Tests require internet access.
        doCheck = false;
      };
    in
    {
      devShells.default = pkgs.mkShell {
        name = "melaffeine-dev";
        packages = [
          rustToolchain
          pkgs.cargo-nextest
          pkgs.cargo-llvm-cov
          cargoDeny
          pkgs.cargo-machete
          pkgs.cargo-mutants
          pkgs.uv
          pkgs.zizmor
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
