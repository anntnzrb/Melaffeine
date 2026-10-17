{ ... }:
{
  perSystem =
    {
      self',
      craneLib,
      commonArgs,
      cargoArtifacts,
      ...
    }:
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
            # AppKit integration requires a user WindowServer session, absent in the Nix sandbox.
            cargoNextestExtraArgs = "--workspace --all-features --no-tests=pass -E 'not binary(main_thread_app)'";
          }
        );
      };
    };
}
