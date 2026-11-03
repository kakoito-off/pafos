{
  description = "pafos: учебная ОС на Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rust = pkgs.rust-bin.nightly.latest.default.override {
        extensions = [ "rust-src" "llvm-tools-preview" ];
      };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [ rust pkgs.cargo-bootimage pkgs.qemu ];
      };
    };
}
