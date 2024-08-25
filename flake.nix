{
  description = "twbar Dev Env";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [
          (import rust-overlay)
          # Fix crash with tooltips: https://github.com/wmww/gtk4-layer-shell/issues/24
          (self: super: {
            gtk4-layer-shell = super.gtk4-layer-shell.overrideAttrs (oldAttrs: {
              nativeBuildInputs = oldAttrs.nativeBuildInputs ++ [ super.wayland-protocols ];
            });
          })
      ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            gcc
            (rust-bin.stable.latest.default.override {
              extensions = [ "cargo" "clippy" "rust-src" "rust-analyzer" ];
            })
            clippy
            gtk4
            gtk4-layer-shell
            vscodium
          ];

          nativeBuildInputs = [
            pkg-config
          ];
        };
      }
    );
}