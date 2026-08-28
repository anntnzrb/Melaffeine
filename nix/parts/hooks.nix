{ inputs, ... }:
{
  imports = [ inputs.git-hooks.flakeModule ];

  perSystem =
    { rustToolchain, ... }:
    {
      pre-commit = {
        check.enable = false;
        settings = {
          hooks = {
            clippy = {
              enable = true;
              packageOverrides.cargo = rustToolchain;
              packageOverrides.clippy = rustToolchain;
            };
            rustfmt = {
              enable = true;
              packageOverrides.rustfmt = rustToolchain;
            };
          };
        };
      };
    };
}
