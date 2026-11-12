{
  description = "Terminal markdown reader";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    inputs:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "armv7l-linux"
        "aarch64-darwin"
      ];

      eachSystem =
        systems: f:
        builtins.foldl' (
          a: s: a // builtins.mapAttrs (k: v: (a.${k} or { }) // { ${s} = v; }) (f s)
        ) { } systems;

      overlay = final: _prev: {
        ink-md = final.callPackage ./nix/package.nix { };
      };
    in
    eachSystem systems (
      system:
      let
        pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [ overlay ];
          config.allowUnfree = true;
        };
      in
      {
        packages = {
          inherit (pkgs) ink-md;
          default = pkgs.ink-md;
        };

        checks = {
          ink-md-tests = pkgs.ink-md.overrideAttrs { doCheck = true; };
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ inputs.self.packages.${system}.default ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];
        };

        formatter = pkgs.nixfmt;
      }
    )
    // {
      overlays.default = overlay;
    };
}
