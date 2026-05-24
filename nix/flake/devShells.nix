{ pkgs }:

{
  default = pkgs.mkShellNoCC {
    packages = with pkgs; [
      just
      clang-tools
      llvmPackages.clang
    ];

    shellHook = ''
      printf 'Melaffeine dev shell: just test && just build\n'
    '';
  };
}
