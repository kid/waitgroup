{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, fenix, flake-utils, naersk, nixpkgs }:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        devToolchain = fenix.packages.${system}.complete;

        kidibox = ((naersk.lib.${system}.override {
          inherit (fenix.packages.${system}.complete) cargo rustc;
        }).buildPackage {
          src = ./.;
        });
      in
      rec {
        # `nix develop`
        devShell = pkgs.mkShell
          {
            nativeBuildInputs = with pkgs; [
              (devToolchain.withComponents [
                "cargo"
                "clippy"
                "rust-src"
                "rustc"
                "rustfmt"
              ])
              fenix.packages.${system}.rust-analyzer
              cargo-edit
              cargo-watch
            ];
          };
      }));
}
